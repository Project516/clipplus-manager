import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import dev.project516.clipplusmanager

Kirigami.ScrollablePage {
    id: page

    required property Backend backend

    signal openFindMusic

    function addFromField() {
        backend.addLinks(field.text);
        field.clear();
        field.forceActiveFocus();
    }

    title: "Downloads"
    actions: [
        Kirigami.Action {
            text: "Find music"
            icon.name: "search"
            onTriggered: page.openFindMusic()
        },
        Kirigami.Action {
            text: "Clear finished"
            icon.name: "edit-clear-all"
            enabled: page.backend.hasFinished
            onTriggered: page.backend.clearFinished()
        }
    ]

    header: Controls.Control {
        padding: Kirigami.Units.largeSpacing
        contentItem: ColumnLayout {
            spacing: Kirigami.Units.smallSpacing

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Controls.TextField {
                    id: field
                    Layout.fillWidth: true
                    placeholderText: "Paste links"
                    inputMethodHints: Qt.ImhUrlCharactersOnly
                    Accessible.name: "Paste links"
                    onAccepted: page.addFromField()
                    Component.onCompleted: forceActiveFocus()
                }

                Controls.Button {
                    text: "Add"
                    icon.name: "list-add"
                    highlighted: field.text.trim() !== ""
                    onClicked: page.addFromField()
                }
            }

            CardMessage {
                Layout.fillWidth: true
                backend: page.backend
            }
        }
    }

    footer: Controls.ToolBar {
        position: Controls.ToolBar.Footer
        visible: page.backend.waiting > 0
        contentItem: RowLayout {
            spacing: Kirigami.Units.smallSpacing
            Kirigami.Icon {
                source: "folder-music"
                implicitWidth: Kirigami.Units.iconSizes.small
                implicitHeight: Kirigami.Units.iconSizes.small
            }
            Controls.Label {
                Layout.fillWidth: true
                elide: Text.ElideRight
                text: page.backend.waiting === 1 ? "1 song waiting for the Sansa" : page.backend.waiting + " songs waiting for the Sansa"
            }
        }
    }

    ListView {
        id: list
        model: page.backend
        spacing: 0
        currentIndex: -1
        reuseItems: true
        delegate: JobDelegate {
            width: ListView.view.width
            onCancelRequested: id => page.backend.cancel(id)
            onRetryRequested: id => page.backend.retry(id)
        }

        Kirigami.PlaceholderMessage {
            anchors.centerIn: parent
            width: parent.width - Kirigami.Units.gridUnit * 4
            visible: list.count === 0
            icon.name: "folder-music"
            text: "Paste a link to get started"
            explanation: "Songs are saved as mp3 and copied to your Sansa when it is plugged in. You can also drop links on the window."
        }
    }
}
