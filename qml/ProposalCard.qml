import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Frame {
    id: card

    property string explanation: ""
    property var edits: []
    property string proposalState: "pending"

    signal applyClicked()
    signal rejectClicked()

    padding: 10
    background: Rectangle {
        radius: 6
        color: card.palette.base
        border.color: card.palette.highlight
        border.width: card.proposalState === "pending" ? 2 : 1
    }

    ColumnLayout {
        width: parent.width
        spacing: 8

        Label {
            text: "Proposed change"
            font.bold: true
        }

        Label {
            Layout.fillWidth: true
            visible: card.explanation !== ""
            text: card.explanation
            textFormat: Text.MarkdownText
            wrapMode: Text.Wrap
        }

        Repeater {
            model: card.edits

            delegate: ColumnLayout {
                id: editView
                required property var modelData
                required property int index
                Layout.fillWidth: true
                spacing: 4

                Label {
                    visible: card.edits.length > 1
                    text: "Edit " + (editView.index + 1) + " of " + card.edits.length
                    opacity: 0.7
                }
                TextArea {
                    Layout.fillWidth: true
                    readOnly: true
                    textFormat: TextEdit.PlainText
                    wrapMode: TextEdit.Wrap
                    font.strikeout: true
                    text: editView.modelData.original
                    background: Rectangle { radius: 4; color: "#26ff5555" }
                }
                TextArea {
                    Layout.fillWidth: true
                    readOnly: true
                    textFormat: TextEdit.PlainText
                    wrapMode: TextEdit.Wrap
                    text: editView.modelData.replacement === "" ? "(delete)" : editView.modelData.replacement
                    background: Rectangle { radius: 4; color: "#2655cc55" }
                }
            }
        }

        RowLayout {
            visible: card.proposalState === "pending"
            Button {
                text: "Apply"
                highlighted: true
                onClicked: card.applyClicked()
            }
            Button {
                text: "Reject"
                onClicked: card.rejectClicked()
            }
        }

        Label {
            visible: card.proposalState !== "pending"
            text: card.proposalState === "applied" ? "✓ Applied" : "✗ Rejected"
            opacity: 0.7
        }
    }
}
