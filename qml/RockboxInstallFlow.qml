import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// The install and update flow. Every input is bound by RockboxPage.
RowLayout {
    id: flow

    // 0 download, 1 copy, 2 unplug and flash, 3 turn on and check, 4 done
    property int step: 0
    // The engine's status text for the running job
    property string detail
    // False while the firmware file waits on a player that is not ejected yet
    property bool ejected: true

    signal ejectRequested
    signal finished

    readonly property var steps: ["Download", "Copy to the player", "Unplug and flash", "Turn on and check"]

    spacing: Kirigami.Units.gridUnit * 2

    ColumnLayout {
        Layout.alignment: Qt.AlignTop
        Layout.preferredWidth: Kirigami.Units.gridUnit * 12
        visible: flow.width > Kirigami.Units.gridUnit * 34
        spacing: Kirigami.Units.largeSpacing
        Accessible.role: Accessible.List
        Accessible.name: "Install steps"

        Repeater {
            model: flow.steps
            delegate: RowLayout {
                id: stepRow
                required property string modelData
                required property int index
                readonly property bool done: flow.step > index
                readonly property bool now: flow.step === index
                spacing: Kirigami.Units.smallSpacing
                Accessible.name: modelData + (done ? ", done" : now ? ", current" : "")

                Kirigami.Icon {
                    source: stepRow.done ? "dialog-ok-apply" : stepRow.now ? "go-next" : "media-record"
                    isMask: true
                    color: stepRow.now ? Kirigami.Theme.highlightColor : stepRow.done ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor
                    implicitWidth: Kirigami.Units.iconSizes.small
                    implicitHeight: Kirigami.Units.iconSizes.small
                }
                Controls.Label {
                    Layout.fillWidth: true
                    text: stepRow.modelData
                    font.bold: stepRow.now
                    color: stepRow.now || stepRow.done ? Kirigami.Theme.textColor : Kirigami.Theme.disabledTextColor
                    elide: Text.ElideRight
                }
            }
        }
    }

    StackLayout {
        Layout.fillWidth: true
        Layout.alignment: Qt.AlignTop
        currentIndex: flow.step

        ColumnLayout {
            spacing: Kirigami.Units.largeSpacing
            Kirigami.Heading {
                level: 3
                text: "Downloading Rockbox"
            }
            Controls.ProgressBar {
                Layout.fillWidth: true
                indeterminate: true
            }
            Controls.Label {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: flow.detail
                color: Kirigami.Theme.disabledTextColor
                textFormat: Text.PlainText
            }
        }

        ColumnLayout {
            spacing: Kirigami.Units.largeSpacing
            Kirigami.Heading {
                level: 3
                text: "Copying files to the player"
            }
            Controls.ProgressBar {
                Layout.fillWidth: true
                indeterminate: true
            }
            Controls.Label {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: flow.detail
                color: Kirigami.Theme.disabledTextColor
                textFormat: Text.PlainText
            }
            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: true
                showCloseButton: false
                type: Kirigami.MessageType.Warning
                text: "Do not unplug the player yet."
            }
        }

        // Blocking step: no cancel, the player is flashing itself.
        ColumnLayout {
            spacing: Kirigami.Units.largeSpacing
            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: !flow.ejected
                showCloseButton: false
                type: Kirigami.MessageType.Warning
                text: "Eject the player, then unplug it so it can flash the bootloader."
                actions: [
                    Kirigami.Action {
                        text: "Eject"
                        icon.name: "media-eject"
                        onTriggered: flow.ejectRequested()
                    }
                ]
            }
            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: flow.ejected
                showCloseButton: false
                type: Kirigami.MessageType.Warning
                text: "Unplug the player now. It flashes the bootloader and powers off by itself in about a minute. Do not touch it."
            }
            Kirigami.Heading {
                level: 3
                text: flow.ejected ? "Unplug the player now" : "Eject the player first"
            }
            Controls.Label {
                Layout.fillWidth: true
                visible: flow.ejected
                wrapMode: Text.WordWrap
                text: "1. Pull out the USB cable.\n2. The player shows an update screen and flashes the bootloader. Do not press any buttons.\n3. When it has powered off, hold the power button to turn it on."
            }
            RowLayout {
                visible: flow.ejected
                spacing: Kirigami.Units.smallSpacing
                Controls.BusyIndicator {
                    implicitWidth: Kirigami.Units.iconSizes.smallMedium
                    implicitHeight: Kirigami.Units.iconSizes.smallMedium
                }
                Controls.Label {
                    text: "Waiting for the player to disconnect"
                }
            }
        }

        ColumnLayout {
            spacing: Kirigami.Units.largeSpacing
            Kirigami.Heading {
                level: 3
                text: "Turn on the player and plug it in again"
            }
            Controls.Label {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: "Rockbox starts after a short loading screen. Plug the player in again and the app checks the install."
            }
            RowLayout {
                spacing: Kirigami.Units.smallSpacing
                Controls.BusyIndicator {
                    implicitWidth: Kirigami.Units.iconSizes.smallMedium
                    implicitHeight: Kirigami.Units.iconSizes.smallMedium
                }
                Controls.Label {
                    text: "Waiting for the player"
                }
            }
        }

        ColumnLayout {
            spacing: Kirigami.Units.largeSpacing
            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: true
                showCloseButton: false
                type: Kirigami.MessageType.Positive
                text: "Rockbox is running on the player."
            }
            Controls.Button {
                text: "Done"
                highlighted: true
                onClicked: flow.finished()
            }
        }
    }
}
