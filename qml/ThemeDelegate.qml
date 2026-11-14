import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Controls.ItemDelegate {
    id: delegate

    property string name
    property string author
    property string size
    property url preview
    property bool installed: false

    signal installRequested

    hoverEnabled: false
    down: false
    highlighted: false

    contentItem: RowLayout {
        spacing: Kirigami.Units.largeSpacing

        // Rockbox screens are small and sharp, so keep the pixels crisp.
        Image {
            Layout.preferredWidth: Kirigami.Units.gridUnit * 7
            Layout.preferredHeight: Kirigami.Units.gridUnit * 3.5
            source: delegate.preview
            fillMode: Image.PreserveAspectFit
            smooth: false
            asynchronous: true
            Accessible.name: "Preview of " + delegate.name
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0
            Controls.Label {
                Layout.fillWidth: true
                text: delegate.name
                font.bold: true
                elide: Text.ElideRight
                textFormat: Text.PlainText
            }
            Controls.Label {
                Layout.fillWidth: true
                text: "by " + delegate.author + (delegate.size !== "" ? " · " + delegate.size : "")
                color: Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
                elide: Text.ElideRight
                textFormat: Text.PlainText
            }
        }

        Kirigami.Badge {
            visible: delegate.installed
            type: Kirigami.Badge.Type.Positive
            text: "Installed"
        }
        Controls.Button {
            visible: !delegate.installed
            text: "Install"
            icon.name: "download"
            onClicked: delegate.installRequested()
        }
    }
}
