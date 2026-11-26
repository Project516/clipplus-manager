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
    property bool canRollBack: false
    // True while a Rockbox job runs
    property bool busy: false
    // "installed" or "pending"
    property string bootloader
    // Each theme: { id, name, author, size, preview, installed }
    property var themes: []
    property bool themesLoaded: false
    property bool databaseAutoUpdate: false
    // False when the setting could not be read from the player
    property bool databaseKnown: false

    signal channelSelected(string channel)
    signal updateRequested
    signal rollbackRequested
    signal reinstallBootloaderRequested
    signal installFontsRequested
    signal updateVoiceRequested
    signal installThemeRequested(string id)
    signal databaseAutoUpdateToggled(bool on)
    signal restoreStockRequested

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
                    text: manage.bootloader === "installed" ? "Installed, Rockbox bootloader" : "Waiting to be flashed"
                }
            }
            RowLayout {
                spacing: Kirigami.Units.smallSpacing
                Controls.Button {
                    text: manage.updateAvailable ? "Update Rockbox" : "Reinstall Rockbox"
                    icon.name: manage.updateAvailable ? "update-none" : "view-refresh"
                    highlighted: manage.updateAvailable
                    enabled: !manage.busy && manage.latestVersion !== ""
                    Controls.ToolTip.text: manage.updateAvailable ? manage.installedVersion + " to " + manage.latestVersion : "Install " + manage.latestVersion + " again"
                    Controls.ToolTip.visible: hovered
                    Controls.ToolTip.delay: Kirigami.Units.toolTipDelay
                    onClicked: manage.updateRequested()
                }
                Controls.Button {
                    text: "Previous build"
                    icon.name: "edit-undo"
                    visible: manage.canRollBack
                    enabled: !manage.busy
                    onClicked: manage.rollbackRequested()
                }
                Controls.Button {
                    text: "Reinstall bootloader"
                    icon.name: "view-refresh"
                    enabled: !manage.busy
                    onClicked: bootloaderConfirm.open()
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
                busy: manage.busy
                onInstallRequested: manage.installThemeRequested(modelData.id)
            }
        }
        Controls.Label {
            visible: manage.themes.length === 0
            text: manage.themesLoaded ? "No themes to show." : "Loading themes"
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
                text: "Some themes need the standard font pack."
            }
            Controls.Button {
                text: "Install fonts"
                icon.name: "download"
                enabled: !manage.busy
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
                text: "Lets the player speak menus and file names."
            }
            Controls.Button {
                text: "Update voice file"
                icon.name: "update-none"
                enabled: !manage.busy
                onClicked: manage.updateVoiceRequested()
            }
        }
    }

    PageSection {
        title: "Database"

        Delegates.SwitchSubtitleDelegate {
            Layout.fillWidth: true
            text: "Update the database automatically"
            subtitle: manage.databaseKnown ? "When on, the player updates at startup if songs changed. Wait for the update to finish before you press play, or Rockbox can crash." : "The setting could not be read from the player. Turn it on or off here to set it."
            enabled: !manage.busy
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
                text: "Writes the SanDisk firmware to the player. Your songs stay on the card."
            }
            Controls.Button {
                text: "Restore stock firmware"
                icon.name: "edit-undo"
                enabled: !manage.busy
                onClicked: confirm.open()
            }
        }
    }

    Kirigami.PromptDialog {
        id: confirm
        title: "Restore the SanDisk firmware?"
        subtitle: "The app writes the file and ejects the player. Unplug it, then turn it on while holding Left (|<<) so the stock firmware starts and flashes the file. Plug it in again and use Remove Rockbox files."
        standardButtons: Kirigami.Dialog.Ok | Kirigami.Dialog.Cancel
        onAccepted: manage.restoreStockRequested()
    }

    Kirigami.PromptDialog {
        id: bootloaderConfirm
        title: "Reinstall the bootloader?"
        subtitle: "The app writes the bootloader file and ejects the player. You then unplug it and it flashes itself in about a minute."
        standardButtons: Kirigami.Dialog.Ok | Kirigami.Dialog.Cancel
        onAccepted: manage.reinstallBootloaderRequested()
    }
}
