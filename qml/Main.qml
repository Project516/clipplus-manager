import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import dev.project516.clipplusmanager

Kirigami.ApplicationWindow {
    id: root

    // Turn on when the Rockbox backend exists.
    property bool rockboxAvailable: false

    // Below this width the sidebar becomes a drawer.
    readonly property bool sidebarFits: width >= Kirigami.Units.gridUnit * 40
    property string section: ""
    readonly property var factories: ({
            "player": playerPage,
            "downloads": downloadsPage,
            "find": findPage,
            "rockbox": rockboxPage,
            "settings": settingsPage
        })

    function goTo(key) {
        if (drawer.modal)
            drawer.drawerOpen = false;
        if (key === section)
            return;
        section = key;
        // Replacing destroys the old page, so each visit starts fresh.
        if (pageStack.depth > 0)
            pageStack.replace(factories[key]);
        else
            pageStack.push(factories[key]);
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
    }

    CardState {
        id: card
        kind: app.cardKind
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
                    text: "Sansa Clip+"
                    font.bold: true
                    elide: Text.ElideRight
                }
                Controls.Label {
                    Layout.fillWidth: true
                    text: card.label
                    elide: Text.ElideRight
                    font: Kirigami.Theme.smallFont
                    color: card.connected || app.cardKind === "safe" ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor
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

    Component {
        id: playerPage
        PlayerPage {
            backend: app
            onOpenDownloads: root.goTo("downloads")
            onOpenRockbox: root.goTo("rockbox")
        }
    }
    Component {
        id: downloadsPage
        DownloadsPage {
            backend: app
            onOpenFindMusic: root.goTo("find")
        }
    }
    Component {
        id: findPage
        FindMusicPage {
            backend: app
            onDownloadStarted: root.goTo("downloads")
        }
    }
    Component {
        id: rockboxPage
        RockboxPage {
            available: root.rockboxAvailable
        }
    }
    Component {
        id: settingsPage
        SettingsPage {
            backend: app
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
