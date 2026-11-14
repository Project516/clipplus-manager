import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import dev.project516.clipplusmanager

Kirigami.ScrollablePage {
    id: page

    required property Backend backend

    // Sizes are bytes, -1 means unknown and hides the card.
    readonly property real internalTotal: backend.internalTotal
    readonly property real internalFree: backend.internalFree
    readonly property real sdTotal: backend.sdTotal
    readonly property real sdFree: backend.sdFree
    // "stock", "rockbox", or "" while unknown. Hides the firmware section.
    readonly property string firmware: backend.firmwareKind === "stock-rockbox" ? "stock" : backend.firmwareKind
    readonly property string firmwareVersion: backend.firmwareVersion
    readonly property bool mtpMode: backend.playerMtp
    readonly property bool bootloaderPending: backend.bootloaderPending

    readonly property CardState cardState: CardState {
        kind: page.backend.cardKind
    }
    readonly property bool hasStorage: (internalTotal > 0 && internalFree >= 0) || (sdTotal > 0 && sdFree >= 0)

    signal openDownloads
    signal openRockbox

    title: "Player"
    actions: [
        Kirigami.Action {
            text: "Eject"
            icon.name: "media-eject"
            enabled: page.backend.cardCanEject
            onTriggered: page.backend.eject()
        }
    ]

    Kirigami.FlexColumn {
        maximumWidth: Kirigami.Units.gridUnit * 44
        spacing: Kirigami.Units.largeSpacing

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.largeSpacing

            Kirigami.Icon {
                source: "multimedia-player"
                implicitWidth: Kirigami.Units.iconSizes.huge
                implicitHeight: Kirigami.Units.iconSizes.huge
            }

            ColumnLayout {
                spacing: Kirigami.Units.smallSpacing
                Kirigami.Heading {
                    level: 1
                    text: "Sansa Clip+"
                }
                RowLayout {
                    spacing: Kirigami.Units.smallSpacing
                    Kirigami.Badge {
                        text: page.mtpMode ? "MTP mode" : page.cardState.label
                        type: page.mtpMode ? Kirigami.Badge.Type.Warning : page.cardState.badgeType
                    }
                    Controls.Label {
                        visible: page.firmware !== ""
                        text: (page.firmware === "rockbox" ? "Rockbox" : "SanDisk firmware") + (page.firmwareVersion !== "" ? " " + page.firmwareVersion : "")
                        color: Kirigami.Theme.disabledTextColor
                        textFormat: Text.PlainText
                    }
                }
            }
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: page.mtpMode
            showCloseButton: false
            type: Kirigami.MessageType.Warning
            text: "The player is in MTP mode, so nothing works yet. On the player, open Settings > System Settings > USB Mode and choose MSC. Then plug it in again."
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: page.backend.flashPending
            showCloseButton: false
            type: Kirigami.MessageType.Warning
            text: "Unplug the player now. It flashes and powers off by itself in about a minute. Do not touch it."
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: page.bootloaderPending && !page.backend.flashPending
            showCloseButton: false
            type: Kirigami.MessageType.Warning
            text: "Eject the player, then unplug it so it can flash the bootloader."
            actions: [
                Kirigami.Action {
                    text: "Eject"
                    icon.name: "media-eject"
                    enabled: !page.backend.rockboxBusy
                    onTriggered: page.backend.ejectPlayer()
                }
            ]
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: page.backend.songsCopied && page.firmware === "rockbox" && !page.backend.databaseAutoUpdate
            type: Kirigami.MessageType.Information
            text: "Songs were copied. On the player, open Settings > General Settings > Database > Update Now so they show up."
            showCloseButton: false
            actions: [
                Kirigami.Action {
                    text: "Got it"
                    icon.name: "dialog-ok"
                    onTriggered: page.backend.dismissSongsCopied()
                }
            ]
        }

        CardMessage {
            Layout.fillWidth: true
            backend: page.backend
        }

        Controls.Label {
            Layout.fillWidth: true
            visible: page.cardState.away && !page.mtpMode
            wrapMode: Text.WordWrap
            text: "Plug in the Sansa Clip+ with its USB cable. Songs copy to it when it appears."
            color: Kirigami.Theme.disabledTextColor
        }

        PageSection {
            title: "Storage"
            visible: page.hasStorage

            GridLayout {
                Layout.fillWidth: true
                columns: page.width > Kirigami.Units.gridUnit * 36 ? 2 : 1
                columnSpacing: Kirigami.Units.largeSpacing
                rowSpacing: Kirigami.Units.largeSpacing

                DriveCard {
                    Layout.fillWidth: true
                    Layout.preferredWidth: 1
                    Layout.alignment: Qt.AlignTop
                    visible: page.internalTotal > 0 && page.internalFree >= 0
                    kind: "Internal storage"
                    iconName: "drive-harddisk"
                    total: page.internalTotal
                    free: page.internalFree
                }
                DriveCard {
                    Layout.fillWidth: true
                    Layout.preferredWidth: 1
                    Layout.alignment: Qt.AlignTop
                    visible: page.sdTotal > 0 && page.sdFree >= 0
                    kind: "microSD card"
                    iconName: "media-flash"
                    total: page.sdTotal
                    free: page.sdFree
                }
            }
        }

        PageSection {
            title: "Firmware"
            visible: page.firmware !== ""

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing
                Controls.Label {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    text: page.firmware === "rockbox" ? "The player runs Rockbox. Manage the build, themes and fonts on the Rockbox page." : "The player runs the SanDisk firmware. Rockbox is open source firmware with themes and more formats."
                }
                Controls.Button {
                    icon.name: "applications-development"
                    text: page.firmware === "rockbox" ? "Manage Rockbox" : "Install Rockbox"
                    onClicked: page.openRockbox()
                }
            }
        }

        PageSection {
            title: "Songs"

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing
                Kirigami.Icon {
                    source: "folder-music"
                    implicitWidth: Kirigami.Units.iconSizes.smallMedium
                    implicitHeight: Kirigami.Units.iconSizes.smallMedium
                }
                Controls.Label {
                    Layout.fillWidth: true
                    text: page.backend.waiting === 0 ? "No songs waiting for the Sansa" : page.backend.waiting === 1 ? "1 song waiting for the Sansa" : page.backend.waiting + " songs waiting for the Sansa"
                }
                Controls.Button {
                    icon.name: "download"
                    text: "Open downloads"
                    onClicked: page.openDownloads()
                }
            }
        }
    }
}
