import { EditorView, basicSetup } from "codemirror";
import { keymap } from "@codemirror/view";
import { defaultKeymap, indentWithTab } from "@codemirror/commands";
import { php } from "@codemirror/lang-php";
import { oneDark } from "@codemirror/theme-one-dark";
import { EditorState } from "@codemirror/state";

export interface CodeEditorOptions {
  parent: HTMLElement;
  initialValue?: string;
  onChange?: (val: string) => void;
  onRun?: () => void;
}

export class CodeEditor {
  private view: EditorView;
  private onChange?: (val: string) => void;

  constructor(options: CodeEditorOptions) {
    this.onChange = options.onChange;

    const runKeymap = keymap.of([
      {
        key: "Mod-Enter",
        run: () => {
          if (options.onRun) {
            options.onRun();
            return true;
          }
          return false;
        },
      },
      indentWithTab,
    ]);

    const customTheme = EditorView.theme(
      {
        "&": {
          height: "100%",
          fontSize: "13.5px",
          fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace",
          backgroundColor: "var(--bg-panel)",
          color: "var(--text-primary)",
        },
        ".cm-scroller": {
          overflow: "auto",
          fontFamily: "inherit",
          lineHeight: "1.6",
        },
        ".cm-content": {
          padding: "12px 16px",
          caretColor: "var(--primary, #4747b8)",
        },
        ".cm-cursor": {
          borderLeftColor: "var(--primary, #4747b8)",
          borderLeftWidth: "2px",
        },
        "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
          backgroundColor: "rgba(71, 71, 184, 0.35) !important",
        },
        ".cm-gutters": {
          backgroundColor: "var(--bg-panel)",
          color: "var(--text-gutter)",
          borderRight: "1px solid var(--border-color)",
          paddingRight: "8px",
          userSelect: "none",
        },
        ".cm-activeLine": {
          backgroundColor: "rgba(255, 255, 255, 0.03)",
        },
        ".cm-activeLineGutter": {
          backgroundColor: "transparent",
          color: "var(--text-primary)",
        },
      },
      { dark: true }
    );

    const startState = EditorState.create({
      doc: options.initialValue || "",
      extensions: [
        basicSetup,
        php(),
        oneDark,
        customTheme,
        keymap.of(defaultKeymap),
        runKeymap,
        EditorView.updateListener.of((update) => {
          if (update.docChanged && this.onChange) {
            this.onChange(update.state.doc.toString());
          }
        }),
      ],
    });

    this.view = new EditorView({
      state: startState,
      parent: options.parent,
    });
  }

  public getValue(): string {
    return this.view.state.doc.toString();
  }

  public setValue(code: string): void {
    if (this.getValue() === code) return;
    this.view.dispatch({
      changes: {
        from: 0,
        to: this.view.state.doc.length,
        insert: code,
      },
    });
  }

  public focus(): void {
    this.view.focus();
  }

  public destroy(): void {
    this.view.destroy();
  }
}
