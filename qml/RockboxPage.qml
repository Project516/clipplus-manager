import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.ScrollablePage {
    id: page

    // False until a Rockbox backend exists. Shows the placeholder only.
    required property bool available

    // Inputs for the Rockbox backend, not wired yet.
    property bool playerReady: false
    property bool installed: false
    property string installedVersion
    property string channel: "dev"
    property string latestVersion
    property bool updateAvailable: false
    // "installed", "missing" or "pending"
    property string bootloader
    property bool fontsInstalled: false
    property string fontsNote
    property string voiceNote
    property var themes: []
    property bool databaseAutoUpdate: false
    // -1 when no install is running, else the RockboxInstallFlow step
    property int installStep: -1
    property string installBuildName
    property real installProgress: -1
    property string installDetail

    signal channelSelected(string channel)
    signal installRequested
    signal updateRequested
    signal reinstallBootloaderRequested
    signal installFontsRequested
    signal updateVoiceRequested
    signal installThemeRequested(string name)
    signal databaseAutoUpdateToggled(bool on)
    signal uninstallRequested
    signal cancelInstallRequested
    signal checkInstallRequested
    signal installFinished
    signal openFolderRequested

    title: "Rockbox"
    actions: [
        Kirigami.Action {
            text: "Open Rockbox folder"
            icon.name: "folder-open"
            visible: page.available && page.installed
            onTriggered: page.openFolderRequested()
        }
    ]

    Kirigami.FlexColumn {
        maximumWidth: Kirigami.Units.gridUnit * 44
        spacing: Kirigami.Units.largeSpacing

        Kirigami.PlaceholderMessage {
            Layout.fillWidth: true
            Layout.topMargin: Kirigami.Units.gridUnit * 4
            visible: !page.available
            icon.name: "applications-development"
            text: "Coming soon"
            explanation: "Install and update Rockbox on the player, and pick themes, fonts and a voice file. This is not available yet."
        }

        Loader {
            Layout.fillWidth: true
            active: page.available
            visible: active
            sourceComponent: ColumnLayout {
                spacing: Kirigami.Units.largeSpacing

                Kirigami.PlaceholderMessage {
                    Layout.fillWidth: true
                    visible: !page.playerReady
                    icon.name: "multimedia-player"
                    text: "Connect the player to manage Rockbox"
                    explanation: "Plug in the Sansa Clip+ with its USB cable. It must not be in MTP mode."
                }

                RockboxInstallFlow {
                    Layout.fillWidth: true
                    visible: page.playerReady && page.installStep >= 0
                    step: page.installStep
                    channel: page.channel
                    buildName: page.installBuildName
                    progress: page.installProgress
                    detail: page.installDetail
                    onChannelSelected: channel => page.channelSelected(channel)
                    onStartRequested: page.installRequested()
                    onCancelRequested: page.cancelInstallRequested()
                    onCheckRequested: page.checkInstallRequested()
                    onFinished: page.installFinished()
                }

                // Stock firmware: offer the install.
                ColumnLayout {
                    Layout.fillWidth: true
                    visible: page.playerReady && page.installStep < 0 && !page.installed
                    spacing: Kirigami.Units.largeSpacing

                    Kirigami.PlaceholderMessage {
                        Layout.fillWidth: true
                        icon.name: "applications-development"
                        text: "Rockbox is not installed"
                        explanation: "The player runs the SanDisk firmware. Rockbox replaces it with open source firmware that plays more formats and has themes. You can go back to stock at any time."
                    }
                    RockboxChannelPicker {
                        Layout.fillWidth: true
                        channel: page.channel
                        onChannelSelected: channel => page.channelSelected(channel)
                    }
                    Controls.Button {
                        text: "Install Rockbox"
                        icon.name: "download"
                        highlighted: true
                        onClicked: page.installRequested()
                    }
                }

                RockboxManage {
                    Layout.fillWidth: true
                    visible: page.playerReady && page.installStep < 0 && page.installed
                    installedVersion: page.installedVersion
                    channel: page.channel
                    latestVersion: page.latestVersion
                    updateAvailable: page.updateAvailable
                    bootloader: page.bootloader
                    fontsInstalled: page.fontsInstalled
                    fontsNote: page.fontsNote
                    voiceNote: page.voiceNote
                    themes: page.themes
                    databaseAutoUpdate: page.databaseAutoUpdate
                    onChannelSelected: channel => page.channelSelected(channel)
                    onUpdateRequested: page.updateRequested()
                    onReinstallBootloaderRequested: page.reinstallBootloaderRequested()
                    onInstallFontsRequested: page.installFontsRequested()
                    onUpdateVoiceRequested: page.updateVoiceRequested()
                    onInstallThemeRequested: name => page.installThemeRequested(name)
                    onDatabaseAutoUpdateToggled: on => page.databaseAutoUpdateToggled(on)
                    onUninstallRequested: page.uninstallRequested()
                }
            }
        }
    }
}
