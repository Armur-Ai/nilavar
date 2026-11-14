//! Command palette actions. The palette lists application actions only; it
//! never runs shell commands or arbitrary code.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    NewDocument,
    NewFolder,
    Save,
    Find,
    FindReplace,
    SearchVault,
    QuickSwitch,
    CommandPalette,
    CloseTab,
    NextTab,
    PrevTab,
    ToggleSidebar,
    TogglePreview,
    ToggleHistory,
    ToggleInfo,
    FocusMode,
    ToggleSoftWrap,
    ToggleLineNumbers,
    ToggleDocMode,
    RenameDocument,
    EditTags,
    TogglePin,
    DeleteDocument,
    ShowTrash,
    ShowAll,
    CopyDocument,
    ImportFiles,
    ImportEncrypted,
    ExportEncrypted,
    ExportPlaintext,
    Backup,
    RestoreBackup,
    ChangePassphrase,
    Settings,
    LockNow,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    RecoveryChecklist,
    SecurityInfo,
    Diagnostics,
}

impl Command {
    pub const ALL: &'static [Command] = &[
        Command::NewDocument,
        Command::NewFolder,
        Command::Save,
        Command::Find,
        Command::FindReplace,
        Command::SearchVault,
        Command::QuickSwitch,
        Command::CloseTab,
        Command::NextTab,
        Command::PrevTab,
        Command::ToggleSidebar,
        Command::TogglePreview,
        Command::ToggleHistory,
        Command::ToggleInfo,
        Command::FocusMode,
        Command::ToggleSoftWrap,
        Command::ToggleLineNumbers,
        Command::ToggleDocMode,
        Command::RenameDocument,
        Command::EditTags,
        Command::TogglePin,
        Command::DeleteDocument,
        Command::ShowTrash,
        Command::ShowAll,
        Command::CopyDocument,
        Command::ImportFiles,
        Command::ImportEncrypted,
        Command::ExportEncrypted,
        Command::ExportPlaintext,
        Command::Backup,
        Command::RestoreBackup,
        Command::ChangePassphrase,
        Command::Settings,
        Command::LockNow,
        Command::ZoomIn,
        Command::ZoomOut,
        Command::ZoomReset,
        Command::RecoveryChecklist,
        Command::SecurityInfo,
        Command::Diagnostics,
    ];

    pub fn label(self) -> &'static str {
        use Command::*;
        match self {
            NewDocument => "New document",
            NewFolder => "New folder",
            Save => "Save now",
            Find => "Find in document",
            FindReplace => "Find and replace",
            SearchVault => "Search vault",
            QuickSwitch => "Go to document…",
            CommandPalette => "Command palette",
            CloseTab => "Close tab",
            NextTab => "Next tab",
            PrevTab => "Previous tab",
            ToggleSidebar => "Toggle sidebar",
            TogglePreview => "Toggle Markdown preview",
            ToggleHistory => "Show version history",
            ToggleInfo => "Show document info",
            FocusMode => "Toggle focus mode",
            ToggleSoftWrap => "Toggle soft wrap",
            ToggleLineNumbers => "Toggle line numbers",
            ToggleDocMode => "Switch Markdown / plain text",
            RenameDocument => "Rename document",
            EditTags => "Edit tags",
            TogglePin => "Pin / unpin document",
            DeleteDocument => "Move document to trash",
            ShowTrash => "Open trash",
            ShowAll => "Show all documents",
            CopyDocument => "Copy document text (auto-clears)",
            ImportFiles => "Import text files…",
            ImportEncrypted => "Import encrypted .age text file…",
            ExportEncrypted => "Export document (encrypted .age)…",
            ExportPlaintext => "Export document as PLAINTEXT…",
            Backup => "Back up vault (encrypted)…",
            RestoreBackup => "Restore backup as new vault…",
            ChangePassphrase => "Change vault passphrase…",
            Settings => "Settings",
            LockNow => "Lock vault",
            ZoomIn => "Zoom in",
            ZoomOut => "Zoom out",
            ZoomReset => "Reset zoom",
            RecoveryChecklist => "Recovery checklist",
            SecurityInfo => "Security & capabilities",
            Diagnostics => "Diagnostic report…",
        }
    }

    pub fn shortcut(self) -> &'static str {
        use Command::*;
        match self {
            NewDocument => "⌘N",
            Save => "⌘S",
            Find => "⌘F",
            FindReplace => "⌥⌘F",
            SearchVault => "⇧⌘F",
            QuickSwitch => "⌘P",
            CommandPalette => "⇧⌘P",
            CloseTab => "⌘W",
            NextTab => "⌃Tab",
            PrevTab => "⌃⇧Tab",
            ToggleSidebar => "⌘\\",
            TogglePreview => "⌘E",
            FocusMode => "⇧⌘\\",
            Settings => "⌘,",
            LockNow => "⇧⌘L",
            ZoomIn => "⌘+",
            ZoomOut => "⌘-",
            ZoomReset => "⌘0",
            _ => "",
        }
    }

    /// Whether the command needs an active document.
    pub fn needs_document(self) -> bool {
        use Command::*;
        matches!(
            self,
            Find | FindReplace
                | CloseTab
                | ToggleDocMode
                | RenameDocument
                | EditTags
                | TogglePin
                | DeleteDocument
                | CopyDocument
                | ExportEncrypted
                | ExportPlaintext
                | ToggleHistory
                | ToggleInfo
        )
    }
}

/// Shortcut text adapted to the platform (⌘ on macOS, Ctrl elsewhere).
pub fn platform_shortcut(s: &str) -> String {
    if cfg!(target_os = "macos") {
        s.to_owned()
    } else {
        s.replace('⌘', "Ctrl+")
            .replace('⇧', "Shift+")
            .replace('⌥', "Alt+")
            .replace('⌃', "Ctrl+")
    }
}
