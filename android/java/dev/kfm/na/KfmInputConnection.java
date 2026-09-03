package dev.kfm.na;

import android.view.KeyEvent;
import android.view.View;
import android.view.inputmethod.BaseInputConnection;

/**
 * IME 连接：commitText 落字 → JNI 直推 Rust。
 *
 * 接上本连接后软键盘的删除/回车走连接回调而非按键队列——不覆写
 * deleteSurroundingText / sendKeyEvent 的话，开中文输入法后退格直接哑火。
 * sendKeyEvent 的可打印键翻字符走 commitText 通道（部分输入法英文字母也
 * 走这里，不翻就丢字）。组词预览（preedit）：setComposingText/
 * finishComposingText 原样转发 native——消费侧按焦点分流（输入栏聚焦
 * → 拼音上栏；终端 → 沿革吞掉，BAR-012 语义不变）。2026-09-01 编辑对齐。
 */
final class KfmInputConnection extends BaseInputConnection {
    KfmInputConnection(View target) {
        super(target, true);
    }

    @Override
    public boolean commitText(CharSequence text, int newCursorPosition) {
        if (text != null) {
            // BAR-054 第五刀：空串也转发——IME 契约里 commit 文本替换当前
            // 选区，空串 = 删选区（输入法工具栏「剪切」的删除半真身，
            // 此前被 length>0 守卫静默吞掉，连日志都没留）
            if (text.length() == 0) {
                KfmImeView.imeLog("commitText 空串（有选区即删选区）");
            }
            KfmImeView.commitText(text.toString());
        }
        return true;
    }

    @Override
    public boolean deleteSurroundingText(int beforeLength, int afterLength) {
        // 软退格：翻成 KEYCODE_DEL 送原生侧映射（ime_queue::key_code_to_bytes）
        // BAR-054 探针：IME 剪切的删除半走这里——记下调用与当时选区状态
        String sel = KfmImeView.selectedText();
        KfmImeView.imeLog("deleteSurroundingText(" + beforeLength + "," + afterLength
                + ") 选区=" + (sel == null ? "null" : sel.length() + "字"));
        int n = Math.min(beforeLength, 64);
        for (int i = 0; i < n; i++) {
            KfmImeView.sendKey(KeyEvent.KEYCODE_DEL);
        }
        return true;
    }

    @Override
    public boolean sendKeyEvent(KeyEvent event) {
        if (event.getAction() == KeyEvent.ACTION_DOWN) {
            int code = event.getKeyCode();
            if (code == KeyEvent.KEYCODE_DEL || code == KeyEvent.KEYCODE_ENTER) {
                KfmImeView.sendKey(code);
            } else {
                // 可打印键（字母/数字/符号）翻字符走落字通道
                int ch = event.getUnicodeChar();
                if (ch != 0) {
                    KfmImeView.commitText(new String(Character.toChars(ch)));
                }
            }
        }
        return true;
    }

    @Override
    public boolean setComposingText(CharSequence text, int newCursorPosition) {
        // 组词预览原样转发（消费侧按焦点分流）；必须收——不收部分输入法罢工
        if (text != null) {
            KfmImeView.composingText(text.toString());
        }
        return true;
    }

    @Override
    public boolean finishComposingText() {
        KfmImeView.finishComposing();
        return true;
    }

    @Override
    public boolean performEditorAction(int actionCode) {
        // BAR-054 探针：动作键经此通道时记下码值（此前静默吞）
        KfmImeView.imeLog("performEditorAction(" + actionCode + ")");
        return true;
    }

    @Override
    public CharSequence getTextBeforeCursor(int n, int flags) {
        // BAR-054 续：IME 内部删除/替换靠它算范围，答空算 0 删个寂寞
        String t = KfmImeView.textBeforeCursor(n);
        KfmImeView.imeLog("getTextBeforeCursor(" + n + ") → " + t.length() + "字");
        return t;
    }

    @Override
    public CharSequence getTextAfterCursor(int n, int flags) {
        String t = KfmImeView.textAfterCursor(n);
        KfmImeView.imeLog("getTextAfterCursor(" + n + ") → " + t.length() + "字");
        return t;
    }

    @Override
    public boolean setSelection(int start, int end) {
        // BAR-054 续：IME 直设光标/选区转发状态核
        KfmImeView.imeLog("setSelection(" + start + "," + end + ")");
        KfmImeView.setSel(start, end);
        return true;
    }

    @Override
    public boolean replaceText(int start, int end, CharSequence text, int newCursorPosition,
            android.view.inputmethod.TextAttribute textAttribute) {
        // BAR-054 续：剪切删除半若走 replaceText(start,end,"") 即此形态。
        // API 35 契约五参（带 TextAttribute，我们不做样式直忽略）；
        // newCursorPosition 是光标落点意愿（>0 落插入尾），状态核
        // replace_range 本就落插入尾——同向，直传即可，探针记下原值
        KfmImeView.imeLog("replaceText(" + start + "," + end + ",\"" + text
                + "\",ncp=" + newCursorPosition + ")");
        KfmImeView.replaceText(start, end, text == null ? "" : text.toString());
        return true;
    }

    @Override
    public boolean deleteSurroundingTextInCodePoints(int beforeLength, int afterLength) {
        // BAR-054 续：deleteSurroundingText 的码点姊妹——IME 剪切删除半
        // 最可疑的暗道（我们不覆写 = 默认实现删内部空 Editable = 删个寂寞）。
        // 探针 + 同 deleteSurroundingText 翻 DEL 键（选区删除逻辑在 Rust 侧）
        KfmImeView.imeLog("deleteSurroundingTextInCodePoints(" + beforeLength
                + "," + afterLength + ")");
        return deleteSurroundingText(beforeLength, afterLength);
    }

    @Override
    public CharSequence getSelectedText(int flags) {
        // BAR-054：输入法工具栏按「剪切」前先探选区，默认实现不认
        // Rust 状态核恒返回空 → 输入法判「无物可剪」不发 cut 事件。
        // 直问状态核拿真实选区；无选区返回 null（契约：无选区 = null）
        CharSequence sel = KfmImeView.selectedText();
        // 探针：剪切全链路第一环——IME 到底来不来问、问到什么
        KfmImeView.imeLog("getSelectedText 探询 → "
                + (sel == null ? "null" : sel.length() + "字"));
        return sel;
    }

    @Override
    public boolean performContextMenuAction(int id) {
        // 输入法工具栏的复制/剪切/粘贴/全选（2026-09-02 曲线救国：
        // 系统剪贴板被 ROM 锁死，这些命令直送 Rust 状态核，不走系统剪贴板）
        String action = null;
        if (id == android.R.id.selectAll) action = "selectAll";
        else if (id == android.R.id.cut) action = "cut";
        else if (id == android.R.id.copy) action = "copy";
        else if (id == android.R.id.paste) action = "paste";
        if (action != null) {
            KfmImeView.contextMenuAction(action);
            return true;
        }
        // BAR-054 第四刀：IME 剪切实锤不走标准四 id——未识别 id 此前静默
        // 漏进 super（默认实现删内部空 Editable = 删个寂寞）。记下数值，
        // 厂商私有 id 抓到即现形
        KfmImeView.imeLog("context-menu 未识别 id=" + id);
        return super.performContextMenuAction(id);
    }

    @Override
    public boolean beginBatchEdit() {
        // BAR-054 探针：IME 批量删除/替换常以 begin/end 包裹，单独成环
        KfmImeView.imeLog("beginBatchEdit");
        return super.beginBatchEdit();
    }

    @Override
    public boolean endBatchEdit() {
        KfmImeView.imeLog("endBatchEdit");
        return super.endBatchEdit();
    }

    @Override
    public boolean commitCorrection(android.view.inputmethod.CorrectionInfo correctionInfo) {
        // BAR-054 探针：纠错提交也是潜在的文本改写暗道
        KfmImeView.imeLog("commitCorrection");
        return super.commitCorrection(correctionInfo);
    }

    @Override
    public boolean performPrivateCommand(String action, android.os.Bundle data) {
        // BAR-054 探针：厂商私有指令通道——剪切若走这里 action 名即身份
        KfmImeView.imeLog("performPrivateCommand(\"" + action + "\")");
        return super.performPrivateCommand(action, data);
    }

    @Override
    public boolean setComposingRegion(int start, int end) {
        // BAR-054 探针：组词区直设——只记不动（默认实现改内部空 Editable 无害）
        KfmImeView.imeLog("setComposingRegion(" + start + "," + end + ")");
        return super.setComposingRegion(start, end);
    }
}
