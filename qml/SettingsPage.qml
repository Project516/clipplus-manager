import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.kirigami.delegates as Delegates
import dev.project516.clipplusmanager

Kirigami.ScrollablePage {
    id: page

    required property Backend backend

    title: "Settings"

    Kirigami.FlexColumn {
        maximumWidth: Kirigami.Units.gridUnit * 40
        spacing: Kirigami.Units.largeSpacing

        PageSection {
            title: "Downloads"

            Delegates.SwitchSubtitleDelegate {
                Layout.fillWidth: true
                text: "Remove sponsor segments"
                subtitle: "Cuts sponsor parts out of downloaded songs."
                checked: page.backend.sponsorBlock
                onToggled: page.backend.setSponsorBlockEnabled(checked)
            }
        }

        PageSection {
            title: "Updates"

            Controls.Label {
                Layout.fillWidth: true
                text: "Version " + page.backend.appVersion()
            }

            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: page.backend.updatedVersion !== ""
                showCloseButton: true
                type: Kirigami.MessageType.Positive
                text: "Updated to " + page.backend.updatedVersion + ". Restart to use it"
                actions: [
                    Kirigami.Action {
                        text: "Restart"
                        icon.name: "view-refresh"
                        onTriggered: {
                            if (page.backend.restart())
                                Qt.quit();
                        }
                    }
                ]
            }

            Kirigami.InlineMessage {
                id: updateMessage
                Layout.fillWidth: true
                // Set on change, since the close button clears `visible`.
                visible: false
                Component.onCompleted: visible = page.backend.updateNote !== ""
                showCloseButton: !page.backend.checkingUpdates
                type: Kirigami.MessageType.Information
                text: page.backend.updateNote
                Connections {
                    target: page.backend
                    function onUpdateNoteChanged() {
                        updateMessage.visible = page.backend.updateNote !== "";
                    }
                }
            }

            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: page.backend.restartError !== ""
                type: Kirigami.MessageType.Error
                text: page.backend.restartError
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing
                Controls.Label {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    text: "Check for updates to the app and managed tools."
                }
                Controls.BusyIndicator {
                    running: page.backend.checkingUpdates
                    visible: running
                    implicitWidth: Kirigami.Units.iconSizes.smallMedium
                    implicitHeight: Kirigami.Units.iconSizes.smallMedium
                }
                Controls.Button {
                    text: "Check for updates"
                    icon.name: "update-none"
                    enabled: !page.backend.checkingUpdates
                    onClicked: page.backend.checkUpdates()
                }
            }
        }

        PageSection {
            title: "Managed tools"

            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: page.backend.toolsNote !== ""
                showCloseButton: false
                type: Kirigami.MessageType.Information
                text: page.backend.toolsNote
            }

            Controls.Label {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: "ffmpeg, yt-dlp and deno update on their own, and never while a download uses them."
                color: Kirigami.Theme.disabledTextColor
            }
        }
    }
}
