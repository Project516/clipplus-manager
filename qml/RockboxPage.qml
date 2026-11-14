import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import dev.project516.clipplusmanager

Kirigami.ScrollablePage {
    id: page

    required property Backend backend

    readonly property bool mtp: backend.playerMtp
    // The internal drive is mounted and readable.
    readonly property bool playerReady: backend.playerConnected && !mtp && backend.internalTotal >= 0
    readonly property bool installed: backend.firmwareKind === "rockbox"
    // The stock firmware runs and a .rockbox folder is left on the player.
    readonly property bool leftovers: playerReady && backend.firmwareKind === "stock-rockbox"
    readonly property var themes: parseThemes(backend.themeList)

    // The user started the first install on this page.
    property bool installStarted: false
    // The player went through the unplug step.
    property bool flashSeen: false
    // The player was told to go back to stock. The install flow does not apply.
    property bool restoring: false
    property bool themesLoaded: false

    readonly property bool flowActive: !restoring && !backend.rockboxFailed && (installStarted || flashSeen || backend.flashPending || (backend.bootloaderPending && !installed))
    readonly property int rawStep: {
        if (backend.rockboxBusy)
            return backend.rockboxPhase === "downloading" ? 0 : 1;
        if (backend.flashPending || backend.bootloaderPending)
            return 2;
        if (flashSeen)
            return installed ? 4 : 3;
        return 0;
    }
    // Never goes back while the flow runs, so the later download of the bootloader keeps the copy step.
    property int stepFloor: 0
    readonly property int installStep: Math.max(rawStep, stepFloor)

    title: "Rockbox"

    function parseThemes(json) {
        try {
            return JSON.parse(json || "[]");
        } catch (e) {
            return [];
        }
    }

    function finishFlow() {
        installStarted = false;
        flashSeen = false;
        restoring = false;
        stepFloor = 0;
    }

    onRawStepChanged: {
        if (rawStep > stepFloor && flowActive)
            stepFloor = rawStep;
        if (rawStep === 2 && !restoring)
            flashSeen = true;
    }
    // The page is created on the first visit, so this runs once.
    Component.onCompleted: backend.loadRockboxThemes()

    Connections {
        target: page.backend
        function onThemeListChanged() {
            page.themesLoaded = true;
        }
        function onFlashPendingChanged() {
            if (!page.backend.flashPending)
                page.restoring = false;
        }
        function onRockboxPhaseChanged() {
            if (page.backend.rockboxPhase === "failed")
                page.finishFlow();
        }
        function onRockboxChannelChanged() {
            // The engine dropped its theme cache, so the old list cannot install.
            page.themesLoaded = false;
            page.backend.loadRockboxThemes();
        }
    }

    actions: [
        Kirigami.Action {
            text: "Check for updates"
            icon.name: "view-refresh"
            enabled: page.installed && !page.backend.rockboxBusy
            onTriggered: page.backend.rockboxCheck()
        }
    ]

    Kirigami.FlexColumn {
        maximumWidth: Kirigami.Units.gridUnit * 44
        spacing: Kirigami.Units.largeSpacing

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: page.backend.rockboxStatus !== "" && !page.backend.rockboxBusy && !page.flowActive
            showCloseButton: false
            type: page.backend.rockboxFailed ? Kirigami.MessageType.Error : Kirigami.MessageType.Information
            text: page.backend.rockboxStatus
        }

        Controls.ProgressBar {
            Layout.fillWidth: true
            visible: page.backend.rockboxBusy && !page.flowActive
            indeterminate: true
        }
        Controls.Label {
            Layout.fillWidth: true
            visible: page.backend.rockboxBusy && !page.flowActive
            text: page.backend.rockboxStatus
            color: Kirigami.Theme.disabledTextColor
            wrapMode: Text.WordWrap
            textFormat: Text.PlainText
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: page.mtp
            showCloseButton: false
            type: Kirigami.MessageType.Warning
            text: "The player is in MTP mode. On the player, open Settings > System Settings > USB Mode and choose MSC. Then plug it in again."
        }

        Kirigami.PlaceholderMessage {
            Layout.fillWidth: true
            visible: !page.playerReady && !page.flowActive && !page.mtp
            icon.name: "multimedia-player"
            text: "Connect the player to manage Rockbox"
            explanation: "Plug in the Sansa Clip+ with its USB cable. It must not be in MTP mode."
        }

        RockboxInstallFlow {
            Layout.fillWidth: true
            visible: page.flowActive
            step: page.installStep
            detail: page.backend.rockboxStatus
            ejected: page.backend.flashPending
            onEjectRequested: page.backend.ejectPlayer()
            onFinished: page.finishFlow()
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            visible: page.backend.bootloaderPending && !page.flowActive && !page.backend.rockboxBusy
            showCloseButton: false
            type: Kirigami.MessageType.Warning
            text: "Eject the player, then unplug it so it can flash the bootloader."
            actions: [
                Kirigami.Action {
                    text: "Eject"
                    icon.name: "media-eject"
                    onTriggered: page.backend.ejectPlayer()
                }
            ]
        }

        // Stock firmware: offer the install.
        ColumnLayout {
            Layout.fillWidth: true
            visible: page.playerReady && !page.flowActive && !page.installed
            spacing: Kirigami.Units.largeSpacing

            Kirigami.PlaceholderMessage {
                Layout.fillWidth: true
                icon.name: "applications-development"
                text: "Rockbox is not installed"
                explanation: "The player runs the SanDisk firmware. Rockbox replaces it with open source firmware that plays more formats and has themes. You can go back to stock at any time."
            }
            RockboxChannelPicker {
                Layout.fillWidth: true
                channel: page.backend.rockboxChannel
                onChannelSelected: channel => page.backend.chooseRockboxChannel(channel)
            }
            Controls.Button {
                text: "Install Rockbox"
                icon.name: "download"
                highlighted: true
                enabled: !page.backend.rockboxBusy
                onClicked: {
                    page.installStarted = true;
                    page.backend.rockboxInstallFirst();
                }
            }
        }

        PageSection {
            title: "Rockbox files on the player"
            visible: page.leftovers && !page.flowActive

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing
                Controls.Label {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    text: "The player has a .rockbox folder but does not run Rockbox. If you restored the stock firmware, remove the folder."
                }
                Controls.Button {
                    text: "Remove Rockbox files"
                    icon.name: "edit-delete"
                    enabled: !page.backend.rockboxBusy
                    onClicked: page.backend.rockboxUninstall(true)
                }
            }
        }

        RockboxManage {
            Layout.fillWidth: true
            visible: page.playerReady && !page.flowActive && page.installed
            installedVersion: page.backend.firmwareVersion
            channel: page.backend.rockboxChannel
            latestVersion: page.backend.rockboxLatest
            updateAvailable: page.backend.rockboxUpdateAvailable
            canRollBack: page.backend.rockboxCanRollBack
            busy: page.backend.rockboxBusy
            bootloader: page.backend.bootloaderPending ? "pending" : "installed"
            themes: page.themes
            themesLoaded: page.themesLoaded
            databaseAutoUpdate: page.backend.databaseAutoUpdate
            databaseKnown: page.backend.databaseAutoUpdateKnown
            onChannelSelected: channel => page.backend.chooseRockboxChannel(channel)
            onUpdateRequested: page.backend.rockboxUpdate()
            onRollbackRequested: page.backend.rockboxRollback()
            onReinstallBootloaderRequested: page.backend.rockboxInstallBootloader()
            onInstallFontsRequested: page.backend.rockboxInstallFonts()
            onUpdateVoiceRequested: page.backend.rockboxInstallVoice()
            onInstallThemeRequested: id => page.backend.rockboxInstallTheme(id)
            onDatabaseAutoUpdateToggled: on => page.backend.changeDatabaseAutoUpdate(on)
            onRestoreStockRequested: {
                page.restoring = true;
                page.backend.rockboxRestoreStock();
            }
        }
    }
}
