import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.kirigami.delegates as Delegates

ColumnLayout {
    id: picker

    // "dev", "daily" or "release"
    property string channel: "dev"

    signal channelSelected(string channel)

    readonly property var channels: [
        {
            key: "dev",
            name: "Dev",
            note: "Newest changes. May have bugs."
        },
        {
            key: "daily",
            name: "Daily",
            note: "Built once a day from the newest changes."
        },
        {
            key: "release",
            name: "Release",
            note: "Stable and tested."
        }
    ]

    // One row of plain radio buttons, for forms.
    property bool compact: false

    spacing: 0

    Controls.ButtonGroup {
        id: group
    }

    RowLayout {
        visible: picker.compact
        spacing: Kirigami.Units.largeSpacing
        Repeater {
            model: picker.compact ? picker.channels : []
            delegate: Controls.RadioButton {
                required property var modelData
                Controls.ButtonGroup.group: group
                text: modelData.name
                checked: picker.channel === modelData.key
                Controls.ToolTip.text: modelData.note
                Controls.ToolTip.visible: hovered
                Controls.ToolTip.delay: Kirigami.Units.toolTipDelay
                onClicked: picker.channelSelected(modelData.key)
            }
        }
    }

    Repeater {
        model: picker.compact ? [] : picker.channels
        delegate: Delegates.RadioSubtitleDelegate {
            required property var modelData
            Layout.fillWidth: true
            Controls.ButtonGroup.group: group
            text: modelData.name
            subtitle: modelData.note
            checked: picker.channel === modelData.key
            onClicked: picker.channelSelected(modelData.key)
        }
    }
}
