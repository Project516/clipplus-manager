import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.kirigami.delegates as Delegates

// Manage an installed Rockbox. Every input is bound by RockboxPage.
ColumnLayout {
    id: manage

    property string installedVersion
    property string channel: "dev"
    property string latestVersion
    property bool updateAvailable: false
    // "installed", "missing" or "pending"
    property string bootloader
    property bool fontsInstalled: false
    property string fontsNote
    property string voiceNote
    // Each theme: { name, author, size, preview, installed }
    property var themes: []
    property bool databaseAutoUpdate: false

    signal channelSelected(string channel)
    signal updateRequested
    signal reinstallBootloaderRequested
    signal installFontsRequested
    signal updateVoiceRequested
    signal installThemeRequested(string name)
    signal databaseAutoUpdateToggled(bool on)
    signal uninstallRequested

    spacing: Kirigami.Units.largeSpacing

    PageSection {
        title: "Rockbox"

        Kirigami.FormLayout {
            Layout.fillWidth: true

            Controls.Label {
                Kirigami.FormData.label: "Installed:"
                text: manage.installedVersion
                textFormat: Text.PlainText
            }
            RockboxChannelPicker {
                Kirigami.FormData.label: "Channel:"
                compact: true
                channel: manage.channel
                onChannelSelected: channel => manage.channelSelected(channel)
            }
            RowLayout {
                Kirigami.FormData.label: "Latest:"
                spacing: Kirigami.Units.smallSpacing
                Controls.Label {
                    text: manage.latestVersion
                    textFormat: Text.PlainText
                }
                Kirigami.Badge {
                    visible: manage.updateAvailable
                    text: "Update available"
                }
            }
            RowLayout {
                Kirigami.FormData.label: "Bootloader:"
                spacing: Kirigami.Units.smallSpacing
                Kirigami.Icon {
                    source: manage.bootloader === "installed" ? "dialog-ok-apply" : "dialog-warning"
                    implicitWidth: Kirigami.Units.iconSizes.small
                    implicitHeight: Kirigami.Units.iconSizes.small
                }
                Controls.Label {
                    text: manage.bootloader === "installed" ? "Installed, Rockbox bootloader" : manage.bootloader === "pending" ? "Waiting to be flashed" : "Not installed"
                }
            }
            RowLayout {
                spacing: Kirigami.Units.smallSpacing
                Controls.Button {
                    text: "Update Rockbox"
                    icon.name: "update-none"
                    highlighted: manage.updateAvailable
                    onClicked: manage.updateRequested()
                }
                Controls.Button {
                    text: "Reinstall bootloader"
                    icon.name: "view-refresh"
                    onClicked: manage.reinstallBootloaderRequested()
                }
            }
        }
    }

    PageSection {
        title: "Themes"

        Repeater {
            model: manage.themes
            delegate: ThemeDelegate {
                required property var modelData
                Layout.fillWidth: true
                name: modelData.name
                author: modelData.author
                size: modelData.size
                preview: modelData.preview
                installed: modelData.installed
                onInstallRequested: manage.installThemeRequested(modelData.name)
            }
        }
        Controls.Label {
            visible: manage.themes.length === 0
            text: "No themes to show."
            color: Kirigami.Theme.disabledTextColor
        }
    }

    PageSection {
        title: "Fonts"

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing
            Controls.Label {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: manage.fontsNote !== "" ? manage.fontsNote : "Some themes need the standard font pack."
            }
            Controls.Button {
                text: manage.fontsInstalled ? "Reinstall fonts" : "Install fonts"
                icon.name: "download"
                onClicked: manage.installFontsRequested()
            }
        }
    }

    PageSection {
        title: "Voice file"

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing
            Controls.Label {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: manage.voiceNote !== "" ? manage.voiceNote : "Lets the player speak menus and file names."
            }
            Controls.Button {
                text: "Update voice file"
                icon.name: "update-none"
                onClicked: manage.updateVoiceRequested()
            }
        }
    }

    PageSection {
        title: "Database"

        Delegates.SwitchSubtitleDelegate {
            Layout.fillWidth: true
            text: "Update the database automatically"
            subtitle: "New songs show up without opening Database > Update Now on the player."
            checked: manage.databaseAutoUpdate
            onToggled: manage.databaseAutoUpdateToggled(checked)
        }
    }

    PageSection {
        title: "Go back to stock firmware"

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing
            Controls.Label {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: "Removes Rockbox and its bootloader, then restores the SanDisk firmware. Your songs stay on the card."
            }
            Controls.Button {
                text: "Uninstall Rockbox"
                icon.name: "edit-delete"
                onClicked: confirm.open()
            }
        }
    }

    Kirigami.PromptDialog {
        id: confirm
        title: "Uninstall Rockbox?"
        subtitle: "The player goes back to the SanDisk firmware. Your songs stay on the card."
        standardButtons: Kirigami.Dialog.Ok | Kirigami.Dialog.Cancel
        onAccepted: manage.uninstallRequested()
    }
}
