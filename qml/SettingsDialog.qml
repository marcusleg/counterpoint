import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: dialog

    required property var settings

    // The model to reselect after the model list is reloaded.
    property string pendingModel: ""

    // The (baseUrl, apiKey) pair last sent to refreshModels, so unchanged fields don't refetch.
    property string lastRequested: ""

    function refreshModels(force) {
        const requested = baseUrlField.text + "\n" + apiKeyField.text
        if (!force && requested === lastRequested)
            return
        lastRequested = requested
        pendingModel = modelBox.editText
        settings.refreshModels(baseUrlField.text, apiKeyField.text)
    }

    function restoreModel() {
        const index = modelBox.find(pendingModel)
        modelBox.currentIndex = index
        if (index < 0)
            modelBox.editText = pendingModel
    }

    title: "Options"
    modal: true
    closePolicy: Popup.CloseOnEscape
    anchors.centerIn: Overlay.overlay
    width: 560

    onAboutToShow: {
        settings.load()
        baseUrlField.text = settings.baseUrl
        apiKeyField.text = settings.apiKey
        showKey.checked = false
        modelBox.currentIndex = -1
        modelBox.editText = settings.model
        refreshModels(true)
    }

    Connections {
        target: dialog.settings
        // The combo box model rebinds on the same signal; restore after it has.
        function onModelsJsonChanged() { Qt.callLater(dialog.restoreModel) }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 8

        Label { text: "Base URL (OpenAI-compatible)" }
        TextField {
            id: baseUrlField
            Layout.fillWidth: true
            placeholderText: "http://localhost:11434/v1"
            onEditingFinished: dialog.refreshModels(false)
        }

        Label { text: "API key (optional)" }
        RowLayout {
            Layout.fillWidth: true
            TextField {
                id: apiKeyField
                Layout.fillWidth: true
                echoMode: showKey.checked ? TextInput.Normal : TextInput.Password
                placeholderText: "Not needed for local servers"
                onEditingFinished: dialog.refreshModels(false)
            }
            CheckBox { id: showKey; text: "Show" }
        }

        Label { text: "Model" }
        RowLayout {
            Layout.fillWidth: true
            ComboBox {
                id: modelBox
                Layout.fillWidth: true
                editable: true
                model: JSON.parse(dialog.settings.modelsJson)
            }
            Button {
                text: "Refresh"
                enabled: !dialog.settings.loadingModels
                onClicked: dialog.refreshModels(true)
            }
        }

        Label {
            Layout.fillWidth: true
            visible: dialog.settings.loadError.length > 0
            text: dialog.settings.loadError
            wrapMode: Text.Wrap
            color: "#d9534f"
        }

        RowLayout {
            Layout.fillWidth: true
            BusyIndicator {
                running: dialog.settings.loadingModels
                visible: running
                Layout.preferredWidth: 24
                Layout.preferredHeight: 24
            }
            Label {
                Layout.fillWidth: true
                text: dialog.settings.status
                wrapMode: Text.Wrap
                opacity: 0.8
            }
        }

        Label {
            Layout.fillWidth: true
            text: "Stored in " + dialog.settings.settingsPath + " (readable only by you)."
            wrapMode: Text.Wrap
            opacity: 0.6
        }
    }

    footer: Item {
        implicitHeight: buttons.implicitHeight + 24

        RowLayout {
            id: buttons
            anchors.right: parent.right
            anchors.rightMargin: 12
            anchors.verticalCenter: parent.verticalCenter

            Button {
                text: dialog.settings.loadError.length > 0 ? "Overwrite" : "Save"
                highlighted: true
                onClicked: {
                    if (dialog.settings.save(baseUrlField.text, apiKeyField.text, modelBox.editText))
                        dialog.close()
                }
            }
            Button {
                text: "Cancel"
                onClicked: dialog.close()
            }
        }
    }
}
