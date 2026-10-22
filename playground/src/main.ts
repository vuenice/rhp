import { CodeEditor } from "./editor";
import {
  loadApps,
  saveApps,
  getDefaultCode,
  resolveRhp,
  pickDirectory,
  pickFile,
  runScript,
  cancelRun,
} from "./api";
import type { AppItem } from "./types";

class PlaygroundApp {
  private apps: AppItem[] = [];
  private activeIndex: number | null = null; // null = Standalone, number = App index
  private standaloneCode = "";
  private standaloneOutput = "";
  private isRunning = false;

  private editor!: CodeEditor;

  // DOM Elements
  private btnStandalone = document.getElementById("btn-standalone") as HTMLButtonElement;
  private appsList = document.getElementById("apps-list") as HTMLDivElement;
  private btnOpenCreateModal = document.getElementById("btn-open-create-modal") as HTMLButtonElement;

  private headerTitle = document.getElementById("header-title") as HTMLSpanElement;
  private headerBadge = document.getElementById("header-badge") as HTMLSpanElement;
  private headerPath = document.getElementById("header-path") as HTMLSpanElement;
  private executionStatus = document.getElementById("execution-status") as HTMLDivElement;

  private btnSettings = document.getElementById("btn-settings") as HTMLButtonElement;
  private btnFormat = document.getElementById("btn-format") as HTMLButtonElement;
  private btnRun = document.getElementById("btn-run") as HTMLButtonElement;
  private btnCancel = document.getElementById("btn-cancel") as HTMLButtonElement;

  private editorViewRoot = document.getElementById("editor-view-root") as HTMLDivElement;
  private editorContainer = document.getElementById("editor-container") as HTMLDivElement;
  private splitterDivider = document.getElementById("splitter-divider") as HTMLDivElement;
  private workspaceSplit = document.getElementById("workspace-split") as HTMLElement;

  private btnResetCode = document.getElementById("btn-reset-code") as HTMLButtonElement;
  private btnClearCode = document.getElementById("btn-clear-code") as HTMLButtonElement;

  private outputContent = document.getElementById("output-content") as HTMLDivElement;
  private btnCopyOutput = document.getElementById("btn-copy-output") as HTMLButtonElement;
  private btnClearOutput = document.getElementById("btn-clear-output") as HTMLButtonElement;

  // Create Modal Elements
  private modalCreate = document.getElementById("modal-create") as HTMLDivElement;
  private btnCloseCreateModal = document.getElementById("btn-close-create-modal") as HTMLButtonElement;
  private btnCancelCreate = document.getElementById("btn-cancel-create") as HTMLButtonElement;
  private btnConfirmCreate = document.getElementById("btn-confirm-create") as HTMLButtonElement;
  private createAppName = document.getElementById("create-app-name") as HTMLInputElement;
  private createProjectPath = document.getElementById("create-project-path") as HTMLInputElement;
  private createRhpPath = document.getElementById("create-rhp-path") as HTMLInputElement;
  private btnBrowseProject = document.getElementById("btn-browse-project") as HTMLButtonElement;
  private btnBrowseRhp = document.getElementById("btn-browse-rhp") as HTMLButtonElement;
  private createModalError = document.getElementById("create-modal-error") as HTMLDivElement;

  // Settings Modal Elements
  private modalSettings = document.getElementById("modal-settings") as HTMLDivElement;
  private btnCloseSettingsModal = document.getElementById("btn-close-settings-modal") as HTMLButtonElement;
  private btnCancelSettings = document.getElementById("btn-cancel-settings") as HTMLButtonElement;
  private btnConfirmSettings = document.getElementById("btn-confirm-settings") as HTMLButtonElement;
  private btnDeleteConnection = document.getElementById("btn-delete-connection") as HTMLButtonElement;
  private settingsAppName = document.getElementById("settings-app-name") as HTMLInputElement;
  private settingsProjectPath = document.getElementById("settings-project-path") as HTMLInputElement;
  private settingsRhpPath = document.getElementById("settings-rhp-path") as HTMLInputElement;
  private btnSettingsBrowseProject = document.getElementById("btn-settings-browse-project") as HTMLButtonElement;
  private btnSettingsBrowseRhp = document.getElementById("btn-settings-browse-rhp") as HTMLButtonElement;
  private settingsModalError = document.getElementById("settings-modal-error") as HTMLDivElement;

  async init(): Promise<void> {
    this.standaloneCode = await getDefaultCode();
    this.apps = await loadApps();

    // Default connection
    if (this.apps.length > 0) {
      this.activeIndex = 0;
    } else {
      this.activeIndex = null;
    }

    const initialCode = this.activeIndex === null
      ? this.standaloneCode
      : this.apps[this.activeIndex].code || (await getDefaultCode());

    // Initialize CodeMirror 6 Editor
    this.editor = new CodeEditor({
      parent: this.editorViewRoot,
      initialValue: initialCode,
      onChange: (val) => this.onCodeChange(val),
      onRun: () => this.handleRun(),
    });

    this.bindEvents();
    this.render();
  }

  private onCodeChange(newCode: string): void {
    if (this.activeIndex === null) {
      this.standaloneCode = newCode;
    } else if (this.apps[this.activeIndex]) {
      this.apps[this.activeIndex].code = newCode;
      this.saveState();
    }
  }

  private async saveState(): Promise<void> {
    await saveApps(this.apps);
  }

  private render(): void {
    this.renderSidebar();
    this.renderHeader();
    this.renderPanels();
  }

  private renderSidebar(): void {
    // Standalone button
    if (this.activeIndex === null) {
      this.btnStandalone.classList.add("active");
    } else {
      this.btnStandalone.classList.remove("active");
    }

    // Apps list
    this.appsList.innerHTML = "";
    this.apps.forEach((app, idx) => {
      const badge = document.createElement("div");
      badge.className = `app-badge color-${app.color_idx % 4}`;
      if (this.activeIndex === idx) {
        badge.classList.add("active");
      }
      const initial = (app.name || "A").trim().charAt(0).toUpperCase() || "A";
      badge.textContent = initial;
      badge.title = `${app.name} (${app.project_path})`;

      badge.addEventListener("click", () => {
        this.selectConnection(idx);
      });

      this.appsList.appendChild(badge);
    });
  }

  private renderHeader(): void {
    if (this.activeIndex === null) {
      this.headerTitle.textContent = "⚡ RHP Playground";
      this.headerBadge.textContent = "Standalone";
      this.headerBadge.style.display = "inline-block";
      this.headerPath.style.display = "none";
      this.btnSettings.style.display = "none";
    } else {
      const app = this.apps[this.activeIndex];
      this.headerTitle.textContent = app.name;
      this.headerBadge.style.display = "none";
      this.headerPath.textContent = `📁 ${app.project_path}`;
      this.headerPath.title = app.project_path;
      this.headerPath.style.display = "inline-block";
      this.btnSettings.style.display = "inline-flex";
    }
  }

  private renderPanels(): void {
    const activeCode = this.activeIndex === null
      ? this.standaloneCode
      : this.apps[this.activeIndex]?.code || "";

    const activeOutput = this.activeIndex === null
      ? this.standaloneOutput
      : this.apps[this.activeIndex]?.output || "";

    this.editor.setValue(activeCode);
    this.renderOutput(activeOutput);
  }

  private renderOutput(text: string, isError = false): void {
    if (!text || text.trim() === "") {
      this.outputContent.innerHTML = `<span class="output-empty">// Press Run (Ctrl+Enter) to execute...</span>`;
      return;
    }

    let html = "";
    if (isError) {
      html += `<div class="output-error-banner">⚠️ Execution Error</div>`;
    }
    // Escape HTML to prevent injection and preserve whitespace
    const escaped = text
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");

    html += `<div class="output-text">${escaped}</div>`;
    this.outputContent.innerHTML = html;
  }

  private selectConnection(idx: number | null): void {
    if (this.activeIndex === idx) return;
    this.activeIndex = idx;
    this.executionStatus.textContent = "";
    this.render();
  }

  private async handleRun(): Promise<void> {
    if (this.isRunning) return;

    this.isRunning = true;
    this.btnRun.style.display = "none";
    this.btnCancel.style.display = "inline-flex";
    this.executionStatus.className = "execution-status";
    this.executionStatus.innerHTML = `<span class="spinner"></span> Running...`;

    const code = this.editor.getValue();
    const projectPath = this.activeIndex === null ? "" : this.apps[this.activeIndex].project_path;
    const rhpPath = this.activeIndex === null ? "" : this.apps[this.activeIndex].rhp_path;

    try {
      const result = await runScript(code, projectPath, rhpPath);

      if (result.formatted_code) {
        this.editor.setValue(result.formatted_code);
        this.onCodeChange(result.formatted_code);
      }

      const outputText = result.output;
      if (this.activeIndex === null) {
        this.standaloneOutput = outputText;
      } else if (this.apps[this.activeIndex]) {
        this.apps[this.activeIndex].output = outputText;
        await this.saveState();
      }

      this.renderOutput(outputText, result.is_error);

      if (result.is_error) {
        this.executionStatus.className = "execution-status error";
        this.executionStatus.textContent = `Error (${result.elapsed_ms}ms)`;
      } else {
        this.executionStatus.className = "execution-status success";
        this.executionStatus.textContent = `✓ ${result.elapsed_ms}ms`;
      }
    } catch (err) {
      const errStr = String(err);
      this.renderOutput(errStr, true);
      this.executionStatus.className = "execution-status error";
      this.executionStatus.textContent = "Failed";
    } finally {
      this.isRunning = false;
      this.btnRun.style.display = "inline-flex";
      this.btnCancel.style.display = "none";
    }
  }

  private async handleCancel(): Promise<void> {
    await cancelRun();
    this.isRunning = false;
    this.btnRun.style.display = "inline-flex";
    this.btnCancel.style.display = "none";
    this.executionStatus.className = "execution-status error";
    this.executionStatus.textContent = "Cancelled";
  }

  private bindEvents(): void {
    // Standalone selection
    this.btnStandalone.addEventListener("click", () => {
      this.selectConnection(null);
    });

    // Run & Cancel buttons
    this.btnRun.addEventListener("click", () => this.handleRun());
    this.btnCancel.addEventListener("click", () => this.handleCancel());
    this.btnFormat.addEventListener("click", () => this.handleRun());

    // Editor Reset & Clear
    this.btnResetCode.addEventListener("click", async () => {
      if (confirm("Reset current editor code to default snippet?")) {
        const defaultCode = await getDefaultCode();
        this.editor.setValue(defaultCode);
        this.onCodeChange(defaultCode);
      }
    });

    this.btnClearCode.addEventListener("click", () => {
      this.editor.setValue("");
      this.onCodeChange("");
      this.editor.focus();
    });

    // Output Copy & Clear
    this.btnCopyOutput.addEventListener("click", async () => {
      const outputText = this.outputContent.innerText || "";
      if (outputText) {
        await navigator.clipboard.writeText(outputText);
        const originalText = this.btnCopyOutput.textContent;
        this.btnCopyOutput.textContent = "Copied!";
        setTimeout(() => {
          this.btnCopyOutput.textContent = originalText;
        }, 1200);
      }
    });

    this.btnClearOutput.addEventListener("click", () => {
      if (this.activeIndex === null) {
        this.standaloneOutput = "";
      } else if (this.apps[this.activeIndex]) {
        this.apps[this.activeIndex].output = "";
        this.saveState();
      }
      this.renderOutput("");
      this.executionStatus.textContent = "";
    });

    // Settings Modal
    this.btnSettings.addEventListener("click", () => {
      if (this.activeIndex === null || !this.apps[this.activeIndex]) return;
      const app = this.apps[this.activeIndex];
      this.settingsAppName.value = app.name;
      this.settingsProjectPath.value = app.project_path;
      this.settingsRhpPath.value = app.rhp_path;
      this.settingsModalError.style.display = "none";
      this.modalSettings.classList.add("open");
    });

    this.btnCloseSettingsModal.addEventListener("click", () => {
      this.modalSettings.classList.remove("open");
    });
    this.btnCancelSettings.addEventListener("click", () => {
      this.modalSettings.classList.remove("open");
    });

    this.btnSettingsBrowseProject.addEventListener("click", async () => {
      const dir = await pickDirectory();
      if (dir) this.settingsProjectPath.value = dir;
    });

    this.btnSettingsBrowseRhp.addEventListener("click", async () => {
      const file = await pickFile();
      if (file) this.settingsRhpPath.value = file;
    });

    this.btnConfirmSettings.addEventListener("click", async () => {
      if (this.activeIndex === null || !this.apps[this.activeIndex]) return;
      const name = this.settingsAppName.value.trim();
      const projectPath = this.settingsProjectPath.value.trim();
      const rhpPath = this.settingsRhpPath.value.trim();

      if (!projectPath) {
        this.settingsModalError.textContent = "Please specify a project directory path.";
        this.settingsModalError.style.display = "block";
        return;
      }

      this.apps[this.activeIndex].name = name || "App";
      this.apps[this.activeIndex].project_path = projectPath;
      this.apps[this.activeIndex].rhp_path = rhpPath;
      await this.saveState();
      this.modalSettings.classList.remove("open");
      this.render();
    });

    this.btnDeleteConnection.addEventListener("click", async () => {
      if (this.activeIndex === null) return;
      if (confirm(`Delete connection "${this.apps[this.activeIndex].name}"?`)) {
        this.apps.splice(this.activeIndex, 1);
        this.activeIndex = this.apps.length > 0 ? 0 : null;
        await this.saveState();
        this.modalSettings.classList.remove("open");
        this.render();
      }
    });

    // Create Modal
    this.btnOpenCreateModal.addEventListener("click", async () => {
      this.createAppName.value = "";
      this.createProjectPath.value = "";
      this.createRhpPath.value = await resolveRhp("");
      this.createModalError.style.display = "none";
      this.modalCreate.classList.add("open");
    });

    this.btnCloseCreateModal.addEventListener("click", () => {
      this.modalCreate.classList.remove("open");
    });
    this.btnCancelCreate.addEventListener("click", () => {
      this.modalCreate.classList.remove("open");
    });

    // Tab buttons in create modal
    const tabBtns = this.modalCreate.querySelectorAll<HTMLButtonElement>(".tab-btn");
    tabBtns.forEach((btn) => {
      btn.addEventListener("click", () => {
        tabBtns.forEach((b) => b.classList.remove("active"));
        btn.classList.add("active");
        const tab = btn.getAttribute("data-tab");
        const localTab = document.getElementById("tab-content-local")!;
        const dockerTab = document.getElementById("tab-content-docker")!;
        const remoteTab = document.getElementById("tab-content-remote")!;

        localTab.style.display = tab === "local" ? "block" : "none";
        dockerTab.style.display = tab === "docker" ? "block" : "none";
        remoteTab.style.display = tab === "remote" ? "block" : "none";
      });
    });

    document.getElementById("btn-cancel-docker")?.addEventListener("click", () => {
      this.modalCreate.classList.remove("open");
    });
    document.getElementById("btn-cancel-remote")?.addEventListener("click", () => {
      this.modalCreate.classList.remove("open");
    });

    this.btnBrowseProject.addEventListener("click", async () => {
      const dir = await pickDirectory();
      if (dir) {
        this.createProjectPath.value = dir;
        if (!this.createAppName.value.trim()) {
          const parts = dir.replace(/[\\/]+$/, "").split(/[\\/]/);
          const folderName = parts[parts.length - 1];
          if (folderName) this.createAppName.value = folderName;
        }
      }
    });

    this.btnBrowseRhp.addEventListener("click", async () => {
      const file = await pickFile();
      if (file) this.createRhpPath.value = file;
    });

    this.btnConfirmCreate.addEventListener("click", async () => {
      const projectPath = this.createProjectPath.value.trim();
      if (!projectPath) {
        this.createModalError.textContent = "Please specify a project directory path.";
        this.createModalError.style.display = "block";
        return;
      }

      let name = this.createAppName.value.trim();
      if (!name) {
        const parts = projectPath.replace(/[\\/]+$/, "").split(/[\\/]/);
        name = parts[parts.length - 1] || "App";
      }

      const rhpPath = this.createRhpPath.value.trim();
      const newApp: AppItem = {
        id: `app-${Date.now()}`,
        name,
        project_path: projectPath,
        rhp_path: rhpPath,
        code: `echo "Connected to ${name} (RHP environment)!\\n";\n`,
        output: "",
        color_idx: this.apps.length % 4,
      };

      this.apps.push(newApp);
      this.activeIndex = this.apps.length - 1;
      await this.saveState();
      this.modalCreate.classList.remove("open");
      this.render();
    });

    // Draggable Splitter
    this.initSplitter();
  }

  private initSplitter(): void {
    let isDragging = false;

    this.splitterDivider.addEventListener("mousedown", (e) => {
      e.preventDefault();
      isDragging = true;
      this.splitterDivider.classList.add("dragging");
      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";
    });

    window.addEventListener("mousemove", (e) => {
      if (!isDragging) return;
      const rect = this.workspaceSplit.getBoundingClientRect();
      const offset = e.clientX - rect.left;
      const ratio = Math.max(0.15, Math.min(0.85, offset / rect.width));
      this.editorContainer.style.width = `${ratio * 100}%`;
    });

    window.addEventListener("mouseup", () => {
      if (isDragging) {
        isDragging = false;
        this.splitterDivider.classList.remove("dragging");
        document.body.style.cursor = "";
        document.body.style.userSelect = "";
      }
    });
  }
}

// Start application
const app = new PlaygroundApp();
app.init().catch(console.error);
