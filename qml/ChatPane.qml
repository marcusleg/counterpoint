import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Pane {
    id: pane

    required property var chat
    property string selectionText: ""

    signal sendRequested(string text)
    signal applyRequested(int index)

    function submit() {
        const text = input.text.trim()
        if (text === "" || pane.chat.busy)
            return
        pane.sendRequested(text)
        input.clear()
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 8

        RowLayout {
            Layout.fillWidth: true
            Label {
                Layout.fillWidth: true
                text: pane.chat.ghostwriting ? "Ghostwriting" : "Sparring"
                font.bold: true
            }
            Button {
                text: "New conversation"
                onClicked: pane.chat.newConversation()
            }
        }

        Label {
            Layout.fillWidth: true
            text: pane.selectionText === ""
                  ? "No selection — whole document"
                  : "Selection: “" + pane.selectionText.replace(/\s+/g, " ") + "”"
            elide: Label.ElideRight
            opacity: 0.7
        }

        ListView {
            id: messageList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: 8
            model: JSON.parse(pane.chat.messagesJson)
            onCountChanged: Qt.callLater(messageList.positionViewAtEnd)
            ScrollBar.vertical: ScrollBar { policy: size < 1.0 ? ScrollBar.AlwaysOn : ScrollBar.AlwaysOff }

            delegate: Column {
                id: row
                required property var modelData
                required property int index
                width: ListView.view.width

                Frame {
                    visible: row.modelData.kind !== "proposal"
                    width: parent.width
                    padding: 8
                    background: Rectangle {
                        radius: 6
                        color: row.modelData.kind === "user" ? pane.palette.alternateBase
                             : row.modelData.kind === "error" ? "#33ff5555"
                             : pane.palette.base
                        border.color: row.modelData.kind === "error" ? "#ccff5555" : pane.palette.mid
                    }

                    TextEdit {
                        width: parent.width
                        readOnly: true
                        selectByMouse: true
                        wrapMode: TextEdit.Wrap
                        color: pane.palette.text
                        textFormat: row.modelData.kind === "assistant" ? TextEdit.MarkdownText : TextEdit.PlainText
                        text: row.modelData.text ?? ""
                    }
                }

                ProposalCard {
                    visible: row.modelData.kind === "proposal"
                    width: parent.width
                    explanation: row.modelData.explanation ?? ""
                    edits: row.modelData.edits ?? []
                    proposalState: row.modelData.state ?? "pending"
                    onApplyClicked: pane.applyRequested(row.index)
                    onRejectClicked: pane.chat.rejectProposal(row.index)
                }
            }
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(input.implicitHeight, 160)

            TextArea {
                id: input
                textFormat: TextEdit.PlainText
                wrapMode: TextEdit.Wrap
                placeholderText: (pane.chat.ghostwriting ? "Ask for a change…" : "Ask about the text…")
                                 + " (Enter to send, Shift+Enter for a new line)"
                Keys.onPressed: (event) => {
                    if (event.key !== Qt.Key_Return && event.key !== Qt.Key_Enter)
                        return
                    event.accepted = true
                    if (event.modifiers & Qt.ShiftModifier) {
                        input.remove(input.selectionStart, input.selectionEnd)
                        input.insert(input.cursorPosition, "\n")
                    } else {
                        pane.submit()
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            BusyIndicator {
                running: pane.chat.busy
                visible: running
                Layout.preferredWidth: 28
                Layout.preferredHeight: 28
            }
            Label {
                Layout.fillWidth: true
                text: pane.chat.busy ? "Waiting for the LLM…" : ""
                opacity: 0.7
            }
            Button {
                text: "Send"
                enabled: !pane.chat.busy && input.text.trim() !== ""
                onClicked: pane.submit()
            }
        }
    }
}
