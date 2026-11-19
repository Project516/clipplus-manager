import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import dev.project516.clipplusmanager

Kirigami.ApplicationWindow {
    id: root

    // Below this width the sidebar becomes a drawer.
    readonly property bool sidebarFits: width >= Kirigami.Units.gridUnit * 40
    property string section: ""
    // Pages load on first visit, then the pool keeps them alive.
    readonly property var files: ({
            "player": "PlayerPage.qml",
            "downloads": "DownloadsPage.qml",
            "find": "FindMusicPage.qml",
            "rockbox": "RockboxPage.qml",
            "settings": "SettingsPage.qml"
        })

    Kirigami.PagePool {
        id: pool
        cachePages: true
    }

    function wire(key, page) {
        if (key === "player") {
            page.openDownloads.connect(() => goTo("downloads"));
            page.openRockbox.connect(() => goTo("rockbox"));
        } else if (key === "downloads") {
            page.openFindMusic.connect(() => goTo("find"));
        } else if (key === "find") {
            page.downloadStarted.connect(() => goTo("downloads"));
        }
    }

    function goTo(key) {
        if (drawer.modal)
            drawer.drawerOpen = false;
        if (key === section)
            return;
        section = key;
        const url = Qt.resolvedUrl(files[key]);
        const known = pool.pageForUrl(url);
        const props = {
            "backend": app
        };
        if (known) {
            showPage(known);
        } else {
            pool.loadPageWithProperties(url, props, item => {
                wire(key, item);
                showPage(item);
            });
        }
    }

    function showPage(page) {
        if (pageStack.depth > 0)
            pageStack.replace(page);
        else
            pageStack.push(page);
    }

    title: "ClipPlus Manager"
    width: Kirigami.Units.gridUnit * 56
    height: Kirigami.Units.gridUnit * 34
    minimumWidth: Kirigami.Units.gridUnit * 20
    minimumHeight: Kirigami.Units.gridUnit * 18

    onSidebarFitsChanged: drawer.drawerOpen = sidebarFits
    Component.onCompleted: {
        drawer.drawerOpen = sidebarFits;
        goTo("downloads");
    }

    Backend {
        id: app
        onRaiseRequested: {
            root.goTo("downloads");
            root.show();
            root.raise();
            root.requestActivate();
        }
        onLinksSkipped: count => root.showPassiveNotification(count === 1 ? "Already in the list, skipped" : count + " links already in the list, skipped")
    }

    CardState {
        id: card
        kind: app.cardKind
    }

    readonly property bool headerError: app.cardKind === "error" || (app.rockboxFailed && app.playerConnected)
    readonly property string headerState: {
        if (app.flashPending)
            return "Unplug now";
        if (app.rockboxBusy)
            return "Working";
        if (root.headerError && app.cardKind !== "error")
            return "Rockbox problem";
        if (app.playerMtp)
            return "MTP mode";
        if (!card.away || !app.playerConnected)
            return card.label;
        return "Connected";
    }

    Connections {
        target: Qt.application
        function onAboutToQuit() {
            app.shutdown();
        }
    }

    pageStack.columnView.columnResizeMode: Kirigami.ColumnView.SingleColumn
    pageStack.globalToolBar.showNavigationButtons: Kirigami.ApplicationHeaderStyle.NoNavigationButtons

    globalDrawer: Kirigami.GlobalDrawer {
        id: drawer

        modal: !root.sidebarFits
        collapsible: false
        width: Kirigami.Units.gridUnit * 14

        header: RowLayout {
            spacing: Kirigami.Units.largeSpacing

            Kirigami.Icon {
                Layout.margins: Kirigami.Units.largeSpacing
                source: "multimedia-player"
                implicitWidth: Kirigami.Units.iconSizes.large
                implicitHeight: Kirigami.Units.iconSizes.large
            }
            ColumnLayout {
                Layout.fillWidth: true
                Layout.rightMargin: Kirigami.Units.largeSpacing
                spacing: 0
                Controls.Label {
                    Layout.fillWidth: true
                    text: app.firmwareKind === "rockbox" ? "Sansa Clip+ (Rockbox)" : "Sansa Clip+"
                    font.bold: true
                    elide: Text.ElideRight
                }
                Controls.Label {
                    Layout.fillWidth: true
                    text: root.headerState
                    elide: Text.ElideRight
                    font: Kirigami.Theme.smallFont
                    color: root.headerError ? Kirigami.Theme.negativeTextColor : app.playerMtp || app.flashPending ? Kirigami.Theme.neutralTextColor : card.connected || app.cardKind === "safe" || app.playerConnected ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor
                }
            }
        }

        topContent: ColumnLayout {
            Layout.fillWidth: true
            spacing: 0

            NavItem {
                Layout.fillWidth: true
                text: "Player"
                icon.name: "multimedia-player"
                current: root.section === "player"
                onClicked: root.goTo("player")
            }
            NavItem {
                Layout.fillWidth: true
                text: "Downloads"
                icon.name: "download"
                badgeText: app.waiting > 0 ? String(app.waiting) : ""
                current: root.section === "downloads"
                onClicked: root.goTo("downloads")
            }
            NavItem {
                Layout.fillWidth: true
                text: "Find music"
                icon.name: "search"
                current: root.section === "find"
                onClicked: root.goTo("find")
            }
            NavItem {
                Layout.fillWidth: true
                text: "Rockbox"
                icon.name: "applications-development"
                badgeText: app.rockboxUpdateAvailable ? "update" : ""
                current: root.section === "rockbox"
                onClicked: root.goTo("rockbox")
            }
        }

        footer: ColumnLayout {
            spacing: 0

            Kirigami.Separator {
                Layout.fillWidth: true
            }
            NavItem {
                Layout.fillWidth: true
                text: "Settings"
                icon.name: "configure"
                badgeText: app.updatedVersion !== "" ? "1" : ""
                current: root.section === "settings"
                onClicked: root.goTo("settings")
            }
            Controls.Button {
                Layout.fillWidth: true
                Layout.margins: Kirigami.Units.largeSpacing
                text: "Eject player"
                icon.name: "media-eject"
                enabled: app.cardCanEject
                onClicked: app.eject()
            }
        }
    }

    DropArea {
        anchors.fill: parent
        onDropped: drop => {
            if (drop.hasText)
                app.addLinks(drop.text);
            else if (drop.hasUrls)
                app.addLinks(drop.urls.join("\n"));
            root.goTo("downloads");
        }
    }
}
