import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: section

    property string title
    default property alias content: body.data

    Layout.fillWidth: true
    spacing: Kirigami.Units.smallSpacing

    Kirigami.Heading {
        Layout.fillWidth: true
        level: 3
        text: section.title
        visible: section.title !== ""
    }

    ColumnLayout {
        id: body
        Layout.fillWidth: true
        spacing: Kirigami.Units.smallSpacing
    }
}
