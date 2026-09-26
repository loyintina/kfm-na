//! tools.rs — 工具面四件：read_file / write_file / run_command / clock。
//!
//! schema 面（发给模型）与执行面（过 Host）同文件单源。run_command 围栏：
//! ①cwd 锁配置根——纯函数闸 check_command 拒命令文本里的 cd 逃逸，
//!   宿主半在 StdHost（current_dir 恒为配置根）；
//! ②禁 sudo/su——check_command 拒命令位的 sudo/su 调用；
//! ③命令与 stdout/stderr 全量进 wire——agent.rs 的 tool_call/tool_result
//!   事件机械保证（execute 的出入参原样入账）。

use crate::dialect::ToolSpec;
use crate::host::Host;

/// 四件工具的 schema 面（A 档纯装配）。
pub fn tool_specs() -> Vec<ToolSpec> {
    let obj = |props: serde_json::Value, required: &[&str]| {
        serde_json::json!({
            "type": "object",
            "properties": props,
            "required": required,
        })
    };
    vec![
        ToolSpec {
            kind: "function",
            function: crate::dialect::ToolFn {
                name: "read_file",
                description: "读文件全文（UTF-8 文本）。参数 path：绝对路径。",
                parameters: obj(
                    serde_json::json!({"path": {"type": "string", "description": "绝对路径"}}),
                    &["path"],
                ),
            },
        },
        ToolSpec {
            kind: "function",
            function: crate::dialect::ToolFn {
                name: "write_file",
                description: "整写文件（父目录自动建）。参数 path：绝对路径；content：全文。",
                parameters: obj(
                    serde_json::json!({
                        "path": {"type": "string", "description": "绝对路径"},
                        "content": {"type": "string", "description": "文件全文"},
                    }),
                    &["path", "content"],
                ),
            },
        },
        ToolSpec {
            kind: "function",
            function: crate::dialect::ToolFn {
                name: "run_command",
                description: "在工作区根执行 shell 命令（sh -c）。cwd 恒为配置根、\
                              不许 cd 逃逸、禁 sudo/su；stdout/stderr/exit_code 全量回传。",
                parameters: obj(
                    serde_json::json!({"command": {"type": "string", "description": "命令文本"}}),
                    &["command"],
                ),
            },
        },
        ToolSpec {
            kind: "function",
            function: crate::dialect::ToolFn {
                name: "clock",
                description: "取当前 UTC 时间（RFC3339 文本）。无参数。",
                parameters: obj(serde_json::json!({}), &[]),
            },
        },
    ]
}

/// run_command 围栏①②的纯函数闸（A 档）：扫描命令文本，
/// 命令位（句首 / `;` `&` `|` `(` 之后）出现 cd / sudo / su 即拒。
/// 参数位的同名词（echo sudoers）放行——拦的是调用不是字符串。
pub fn check_command(cmd: &str) -> Result<(), String> {
    let bytes = cmd.as_bytes();
    let mut i = 0;
    let mut at_cmd_pos = true; // 句首即命令位
    while i < bytes.len() {
        let b = bytes[i];
        if b == b';' || b == b'&' || b == b'|' || b == b'(' || b == b'\n' || b == b'`' {
            at_cmd_pos = true;
            i += 1;
            continue;
        }
        if b == b' ' || b == b'\t' || b == b'\r' {
            i += 1;
            continue;
        }
        // 一个词的起点
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && !b";&|()`".contains(&bytes[i]) {
            i += 1;
        }
        let word = &cmd[start..i];
        if at_cmd_pos {
            match word {
                "cd" => {
                    return Err("围栏①：cwd 锁配置根，命令文本里的 cd 逃逸被拒".into());
                }
                "sudo" | "su" => {
                    return Err(format!("围栏②：禁 sudo/su 调用（命中 {word}）"));
                }
                _ => {}
            }
            at_cmd_pos = false;
        }
    }
    Ok(())
}

/// 执行一个工具调用，回传「给模型的工具消息文本」。
/// wire 入账（tool_call/tool_result 事件）在 agent.rs——这里只管执行。
pub fn execute(host: &dyn Host, name: &str, arguments_json: &str) -> String {
    match execute_inner(host, name, arguments_json) {
        Ok(s) => s,
        Err(e) => format!("工具错误: {e}"),
    }
}

fn execute_inner(host: &dyn Host, name: &str, arguments_json: &str) -> Result<String, String> {
    let args: serde_json::Value =
        serde_json::from_str(arguments_json).map_err(|e| format!("arguments 非合法 JSON: {e}"))?;
    match name {
        "read_file" => {
            let path = arg_str(&args, "path")?;
            host.read_file(path)
        }
        "write_file" => {
            let path = arg_str(&args, "path")?;
            let content = arg_str(&args, "content")?;
            host.write_file(path, content)?;
            Ok(format!("已写入 {path}（{} 字节）", content.len()))
        }
        "run_command" => {
            let command = arg_str(&args, "command")?;
            check_command(command)?; // 围栏①②
            let out = host.run_command(command)?;
            // 围栏③的模型半：三件套全量回传（wire 半在 agent.rs 事件）
            Ok(serde_json::json!({
                "exit_code": out.exit_code,
                "stdout": out.stdout,
                "stderr": out.stderr,
            })
            .to_string())
        }
        "clock" => Ok(host.now_rfc3339()),
        other => Err(format!("未知工具: {other}")),
    }
}

fn arg_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("缺参数 {key}"))
}
