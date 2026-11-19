import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import dev.project516.clipplusmanager

Kirigami.Page {
    id: page

    required property Backend backend

    // 0 picks a song on MusicBrainz, 1 picks a file on Soulseek.
    property int step: 0
    property bool searched: false
    property string trackTitle: ""
    property bool albumMode: false

    signal downloadStarted

    function goBack() {
        backend.cancelAlbumSearch();
        step = 0;
    }

    Component.onDestruction: backend.cancelAlbumSearch()

    function duration(ms) {
        const seconds = Math.floor(ms / 1000);
        return Math.floor(seconds / 60) + ":" + ("0" + seconds % 60).slice(-2);
    }

    function search() {
        if (query.text.trim() === "" || (albumMode && !backend.soulseekReady))
            return;
        searched = query.text.trim() !== "";
        if (albumMode) {
            trackTitle = query.text.trim();
            backend.searchAlbums(trackTitle);
            step = 1;
        } else {
            backend.searchMusic(query.text);
        }
    }

    title: step === 0 ? "Find music" : (albumMode ? "Choose an album folder" : "Choose a file")
    actions: [
        Kirigami.Action {
            text: "Back"
            icon.name: "go-previous"
            visible: page.step === 1
            onTriggered: page.goBack()
        }
    ]

    Component.onCompleted: {
        backend.soulseekConnect();
        query.forceActiveFocus();
    }

    Connections {
        target: page.backend
        function onAlbumQueued() {
            page.step = 0;
            page.downloadStarted();
        }
        function onSearchResultsChanged() {
            songs.currentIndex = -1;
        }
        function onPeerResultsChanged() {
            files.currentIndex = -1;
        }
    }

    StackLayout {
        anchors.fill: parent
        currentIndex: page.step

        ColumnLayout {
            spacing: Kirigami.Units.smallSpacing

            Controls.ComboBox {
                model: ["Song", "Album"]
                Accessible.name: "Search for"
                onCurrentIndexChanged: page.albumMode = currentIndex === 1
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Controls.TextField {
                    id: query
                    Layout.fillWidth: true
                    placeholderText: page.albumMode ? "Artist and album" : "Artist and song"
                    Accessible.name: placeholderText
                    onAccepted: page.search()
                }
                Controls.Button {
                    text: "Search"
                    icon.name: "search"
                    highlighted: enabled
                    enabled: query.text.trim() !== "" && (!page.albumMode || page.backend.soulseekReady)
                    onClicked: page.search()
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Kirigami.Badge {
                    visible: page.backend.soulseekReady
                    type: Kirigami.Badge.Type.Positive
                    text: "Connected to Soulseek"
                }
                Controls.Label {
                    Layout.fillWidth: true
                    visible: !page.backend.soulseekReady
                    text: page.backend.soulseekStatus === "" ? "Not connected to Soulseek" : page.backend.soulseekStatus
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    color: Kirigami.Theme.disabledTextColor
                }
                Item {
                    Layout.fillWidth: true
                    visible: page.backend.soulseekReady
                }
                Controls.Button {
                    visible: !page.backend.soulseekReady
                    text: "Connect"
                    onClicked: page.backend.soulseekConnect()
                }
            }

            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: !page.albumMode && page.backend.searchError !== ""
                text: page.backend.searchError
                type: Kirigami.MessageType.Error
            }

            Kirigami.Heading {
                level: 4
                text: "Choose a song"
                visible: !page.albumMode && songs.count > 0
            }

            ListView {
                id: songs
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                currentIndex: -1
                keyNavigationEnabled: true
                activeFocusOnTab: true
                model: page.albumMode || page.backend.searchResults === "" ? [] : JSON.parse(page.backend.searchResults)
                Controls.ScrollBar.vertical: Controls.ScrollBar {}
                delegate: Controls.ItemDelegate {
                    required property var modelData
                    required property int index
                    width: ListView.view.width
                    highlighted: ListView.isCurrentItem
                    onClicked: songs.currentIndex = index
                    contentItem: ColumnLayout {
                        spacing: 0
                        Controls.Label {
                            Layout.fillWidth: true
                            text: modelData.title
                            textFormat: Text.PlainText
                            elide: Text.ElideRight
                        }
                        Controls.Label {
                            Layout.fillWidth: true
                            text: modelData.artist + (modelData.release ? " · " + modelData.release : "") + (modelData.length ? " · " + page.duration(modelData.length) : "")
                            textFormat: Text.PlainText
                            elide: Text.ElideRight
                            color: Kirigami.Theme.disabledTextColor
                            font: Kirigami.Theme.smallFont
                        }
                    }
                }

                Controls.BusyIndicator {
                    anchors.centerIn: parent
                    running: !page.albumMode && page.backend.searching
                    visible: running
                }
                Kirigami.PlaceholderMessage {
                    anchors.centerIn: parent
                    width: parent.width - Kirigami.Units.gridUnit * 4
                    visible: page.albumMode || (!page.backend.searching && page.backend.searchError === "" && songs.count === 0)
                    icon.name: "search"
                    text: page.albumMode ? "Search Soulseek for an album" : (page.searched ? "No recordings found" : "Search MusicBrainz for a song")
                    explanation: page.albumMode ? "Choose a folder to download its songs." : (page.searched ? "" : "Then pick a file from Soulseek. It is saved as mp3 and copied to your Sansa.")
                }
            }

            Controls.Button {
                Layout.alignment: Qt.AlignRight
                text: "Find downloads"
                icon.name: "go-next"
                highlighted: enabled
                visible: !page.albumMode
                enabled: page.backend.soulseekReady && songs.currentIndex >= 0 && !page.backend.searching
                onClicked: {
                    const selected = songs.model[songs.currentIndex];
                    page.trackTitle = selected.artist + " · " + selected.title;
                    page.backend.findPeers(songs.currentIndex);
                    page.step = 1;
                }
            }
        }

        ColumnLayout {
            spacing: Kirigami.Units.smallSpacing

            Controls.Label {
                Layout.fillWidth: true
                text: page.trackTitle
                textFormat: Text.PlainText
                elide: Text.ElideRight
                font.bold: true
            }

            Kirigami.InlineMessage {
                Layout.fillWidth: true
                visible: page.backend.peerError !== ""
                text: page.backend.peerError
                type: Kirigami.MessageType.Error
            }

            ListView {
                id: files
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                currentIndex: -1
                keyNavigationEnabled: true
                activeFocusOnTab: true
                model: page.backend.peerResults === "" ? [] : JSON.parse(page.backend.peerResults)
                Controls.ScrollBar.vertical: Controls.ScrollBar {}
                delegate: Controls.ItemDelegate {
                    required property var modelData
                    required property int index
                    width: ListView.view.width
                    highlighted: ListView.isCurrentItem
                    onClicked: files.currentIndex = index
                    contentItem: ColumnLayout {
                        spacing: 0
                        Controls.Label {
                            Layout.fillWidth: true
                            text: modelData.filename
                            textFormat: Text.PlainText
                            elide: Text.ElideLeft
                        }
                        Controls.Label {
                            Layout.fillWidth: true
                            text: {
                                const peer = modelData.username + " · " + (modelData.free_slot ? "free slot" : "queue " + modelData.queue_length);
                                if (page.albumMode)
                                    return peer;
                                return peer + " · " + (modelData.size / 1000000).toFixed(1) + " MB" + (modelData.bitrate ? " · " + modelData.bitrate + " kbps" : "") + (modelData.length ? " · " + page.duration(modelData.length * 1000) : "");
                            }
                            textFormat: Text.PlainText
                            elide: Text.ElideRight
                            color: Kirigami.Theme.disabledTextColor
                            font: Kirigami.Theme.smallFont
                        }
                    }
                }

                Controls.BusyIndicator {
                    anchors.centerIn: parent
                    running: page.backend.peerSearching
                    visible: running
                }
                Kirigami.PlaceholderMessage {
                    anchors.centerIn: parent
                    width: parent.width - Kirigami.Units.gridUnit * 4
                    visible: !page.backend.peerSearching && page.backend.peerError === "" && files.count === 0
                    text: "No downloads found"
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Controls.Button {
                    text: "Back"
                    icon.name: "go-previous"
                    onClicked: page.goBack()
                }
                Item {
                    Layout.fillWidth: true
                }
                Controls.Button {
                    visible: !page.albumMode
                    text: "Download full album"
                    icon.name: "download"
                    enabled: !page.backend.peerSearching && files.currentIndex >= 0
                    onClicked: page.backend.downloadPeerAlbum(files.currentIndex)
                }
                Controls.Button {
                    text: page.albumMode ? "Download album" : "Download"
                    icon.name: "download"
                    highlighted: enabled
                    enabled: !page.backend.peerSearching && files.currentIndex >= 0
                    onClicked: {
                        if (page.albumMode) {
                            page.backend.downloadAlbum(files.currentIndex);
                        } else {
                            page.backend.downloadPeer(files.currentIndex);
                            page.step = 0;
                            page.downloadStarted();
                        }
                    }
                }
            }
        }
    }
}
