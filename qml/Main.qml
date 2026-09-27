import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import Counterpoint

ApplicationWindow {
    id: window
    width: 1400
    height: 850
    visible: true
    title: (editor.textDocument.modified ? "• " : "") + (documentController.fileName || "Untitled") + " — Counterpoint"

    // Follow the desktop's dark colour scheme. The platform theme reports the scheme but supplies no
    // dark palette, so provide one; `undefined` keeps the theme's own colours in light mode.
    readonly property bool darkScheme: Application.styleHints.colorScheme === Qt.ColorScheme.Dark
    palette.window: darkScheme ? "#2d2d2d" : undefined
    palette.active.windowText: darkScheme ? "#e6e6e6" : undefined
    palette.inactive.windowText: darkScheme ? "#e6e6e6" : undefined
    palette.disabled.windowText: darkScheme ? "#7a7a7a" : undefined
    palette.base: darkScheme ? "#1e1e1e" : undefined
    palette.alternateBase: darkScheme ? "#2a2a2a" : undefined
    palette.toolTipBase: darkScheme ? "#3a3a3a" : undefined
    palette.toolTipText: darkScheme ? "#e6e6e6" : undefined
    palette.placeholderText: darkScheme ? "#8a8a8a" : undefined
    palette.active.text: darkScheme ? "#e6e6e6" : undefined
    palette.inactive.text: darkScheme ? "#e6e6e6" : undefined
    palette.disabled.text: darkScheme ? "#7a7a7a" : undefined
    palette.button: darkScheme ? "#353535" : undefined
    palette.active.buttonText: darkScheme ? "#e6e6e6" : undefined
    palette.inactive.buttonText: darkScheme ? "#e6e6e6" : undefined
    palette.disabled.buttonText: darkScheme ? "#7a7a7a" : undefined
    palette.brightText: darkScheme ? "#ff5555" : undefined
    palette.light: darkScheme ? "#505050" : undefined
    palette.midlight: darkScheme ? "#3f3f3f" : undefined
    palette.mid: darkScheme ? "#5a5a5a" : undefined
    palette.dark: darkScheme ? "#1a1a1a" : undefined
    palette.shadow: darkScheme ? "#000000" : undefined
    palette.highlight: darkScheme ? "#3d7fd6" : undefined
    palette.highlightedText: darkScheme ? "#ffffff" : undefined
    palette.link: darkScheme ? "#6fa8ff" : undefined
    palette.linkVisited: darkScheme ? "#b48ead" : undefined

    // "open" or "close" while the unsaved-changes dialog or Save As dialog is pending.
    property string pendingAction: ""
    property bool closeConfirmed: false

    function requestOpen() {
        if (editor.textDocument.modified) {
            pendingAction = "open"
            unsavedDialog.open()
        } else {
            openDialog.open()
        }
    }

    function save() {
        if (documentController.filePath === "")
            saveAs()
        else
            documentController.save(editor.textDocument)
    }

    function saveAs() {
        pendingAction = ""
        saveDialog.open()
    }

    function continuePendingAction() {
        const action = pendingAction
        pendingAction = ""
        if (action === "open") {
            openDialog.open()
        } else if (action === "close") {
            closeConfirmed = true
            window.close()
        }
    }

    function sendChat(text) {
        const selection = editor.selectionEnd > editor.selectionStart
            ? documentController.selectionMarkdown(editor.textDocument, editor.selectionStart, editor.selectionEnd)
            : ""
        chatController.send(text, documentController.markdown(editor.textDocument), selection)
    }

    function applyProposal(index) {
        chatController.applyProposal(index, documentController.markdown(editor.textDocument))
    }

    onClosing: (close) => {
        if (editor.textDocument.modified && !closeConfirmed) {
            close.accepted = false
            pendingAction = "close"
            unsavedDialog.open()
        }
    }

    DocumentController {
        id: documentController
        onErrorOccurred: (message) => {
            errorDialog.text = message
            errorDialog.open()
        }
    }

    ChatController {
        id: chatController
        onProposalApplied: (markdown) => documentController.setMarkdownUndoable(editor.textDocument, markdown)
    }

    SettingsController {
        id: settingsController
    }

    SettingsDialog {
        id: settingsDialog
        settings: settingsController
    }

    menuBar: MenuBar {
        Menu {
            title: "&File"
            Action { text: "&Open…"; shortcut: StandardKey.Open; onTriggered: window.requestOpen() }
            Action { text: "&Save"; shortcut: StandardKey.Save; onTriggered: window.save() }
            Action { text: "Save &As…"; shortcut: StandardKey.SaveAs; onTriggered: window.saveAs() }
            MenuSeparator {}
            Action { text: "&Quit"; shortcut: StandardKey.Quit; onTriggered: window.close() }
        }
        Menu {
            title: "&Tools"
            Action { text: "&Options…"; onTriggered: settingsDialog.open() }
        }
    }

    header: ToolBar {
        RowLayout {
            anchors.fill: parent
            Label {
                text: (editor.textDocument.modified ? "• " : "") + (documentController.fileName || "Untitled")
                elide: Label.ElideMiddle
                Layout.fillWidth: true
                Layout.leftMargin: 12
            }
            ToolSeparator {}
            Label { text: "Mode:" }
            ButtonGroup { id: modeGroup }
            ToolButton {
                text: "Sparring"
                checkable: true
                checked: !chatController.ghostwriting
                ButtonGroup.group: modeGroup
                onClicked: chatController.ghostwriting = false
                ToolTip.visible: hovered
                ToolTip.text: "The LLM can read the document but not change it"
            }
            ToolButton {
                text: "Ghostwriting"
                checkable: true
                checked: chatController.ghostwriting
                ButtonGroup.group: modeGroup
                onClicked: chatController.ghostwriting = true
                ToolTip.visible: hovered
                ToolTip.text: "The LLM can propose changes that you apply or reject"
            }
        }
    }

    SplitView {
        anchors.fill: parent
        orientation: Qt.Horizontal

        ScrollView {
            SplitView.fillWidth: true
            SplitView.minimumWidth: 300
            ScrollBar.vertical.policy: ScrollBar.vertical.size < 1.0 ? ScrollBar.AlwaysOn : ScrollBar.AlwaysOff

            TextArea {
                id: editor
                textFormat: TextEdit.PlainText
                wrapMode: TextEdit.Wrap
                selectByMouse: true
                // Keep the highlight visible while typing in the chat pane.
                persistentSelection: true
                font.pointSize: 12
                leftPadding: 24
                rightPadding: 24
                topPadding: 16
                bottomPadding: 16
                placeholderText: "Open a Markdown file or start writing…"
                Component.onCompleted: documentController.attachHighlighter(editor.textDocument)

                // QML's default Shift+Enter behaviour inserts a U+2028 line separator, which the
                // highlighter (and Markdown itself) treats as part of the same block. Insert a
                // real newline instead, so the source stays exact Markdown.
                Keys.onPressed: (event) => {
                    if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter)
                            && (event.modifiers & Qt.ShiftModifier)) {
                        editor.remove(editor.selectionStart, editor.selectionEnd)
                        editor.insert(editor.cursorPosition, "\n")
                        event.accepted = true
                    }
                }
            }
        }

        ChatPane {
            SplitView.preferredWidth: 440
            SplitView.minimumWidth: 280
            chat: chatController
            selectionText: editor.selectedText
            onSendRequested: (text) => window.sendChat(text)
            onApplyRequested: (index) => window.applyProposal(index)
        }
    }

    FileDialog {
        id: openDialog
        title: "Open Markdown file"
        nameFilters: ["Markdown files (*.md *.markdown)", "All files (*)"]
        onAccepted: documentController.open(editor.textDocument, selectedFile)
    }

    FileDialog {
        id: saveDialog
        title: "Save Markdown file"
        fileMode: FileDialog.SaveFile
        defaultSuffix: "md"
        nameFilters: ["Markdown files (*.md *.markdown)", "All files (*)"]
        onAccepted: {
            if (documentController.saveAs(editor.textDocument, selectedFile) && window.pendingAction !== "")
                window.continuePendingAction()
            else
                window.pendingAction = ""
        }
        onRejected: window.pendingAction = ""
    }

    MessageDialog {
        id: unsavedDialog
        title: "Unsaved changes"
        text: "The document has unsaved changes."
        informativeText: "Do you want to save them first?"
        buttons: MessageDialog.Save | MessageDialog.Discard | MessageDialog.Cancel
        onButtonClicked: (button, role) => {
            if (button === MessageDialog.Save) {
                if (documentController.filePath === "")
                    saveDialog.open() // continues the pending action once saved
                else if (documentController.save(editor.textDocument))
                    window.continuePendingAction()
                else
                    window.pendingAction = ""
            } else if (button === MessageDialog.Discard) {
                window.continuePendingAction()
            } else {
                window.pendingAction = ""
            }
        }
    }

    MessageDialog {
        id: errorDialog
        title: "Error"
        buttons: MessageDialog.Ok
    }
}
