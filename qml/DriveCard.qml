import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Sizes are in bytes. A negative value means the size is unknown.
Kirigami.AbstractCard {
    id: card

    property string kind
    property string volumeName
    property string iconName: "drive-harddisk"
    property real total: -1
    property real free: -1

    readonly property bool known: total > 0 && free >= 0 && free <= total

    function size(bytes) {
        return Qt.locale().formattedDataSize(bytes, 1, Locale.DataSizeSIFormat);
    }

    contentItem: ColumnLayout {
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            spacing: Kirigami.Units.largeSpacing
            Kirigami.Icon {
                source: card.iconName
                implicitWidth: Kirigami.Units.iconSizes.medium
                implicitHeight: Kirigami.Units.iconSizes.medium
            }
            ColumnLayout {
                spacing: 0
                Controls.Label {
                    text: card.kind
                    color: Kirigami.Theme.disabledTextColor
                    font: Kirigami.Theme.smallFont
                }
                Controls.Label {
                    text: card.volumeName
                    visible: text !== ""
                    font.bold: true
                    textFormat: Text.PlainText
                }
            }
        }

        Controls.ProgressBar {
            Layout.fillWidth: true
            from: 0
            to: 1
            value: card.known ? (card.total - card.free) / card.total : 0
            Accessible.name: card.kind + " used"
        }

        Controls.Label {
            Layout.fillWidth: true
            text: card.known ? card.size(card.free) + " free of " + card.size(card.total) : ""
            color: Kirigami.Theme.disabledTextColor
            font: Kirigami.Theme.smallFont
        }
    }
}
