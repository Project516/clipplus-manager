import QtQuick
import QtQuick.Controls as Controls
import org.kde.kirigami as Kirigami

Controls.ItemDelegate {
    id: item

    property string badgeText: ""
    property bool current: false

    focusPolicy: Qt.StrongFocus
    highlighted: current
    Keys.onReturnPressed: clicked()
    Keys.onEnterPressed: clicked()
    rightPadding: badge.visible ? badge.width + Kirigami.Units.largeSpacing * 2 : padding
    Accessible.name: badgeText === "" ? text : text + ", " + badgeText

    Kirigami.Badge {
        id: badge
        anchors.right: parent.right
        anchors.rightMargin: Kirigami.Units.largeSpacing
        anchors.verticalCenter: parent.verticalCenter
        visible: item.badgeText !== ""
        text: item.badgeText
    }
}
