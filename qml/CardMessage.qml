import QtQuick
import org.kde.kirigami as Kirigami
import dev.project516.clipplusmanager

Kirigami.InlineMessage {
    id: message

    required property Backend backend

    visible: backend.cardKind !== "" && backend.cardKind !== "none"
    showCloseButton: false
    text: backend.cardText
    type: {
        switch (backend.cardKind) {
        case "error":
            return Kirigami.MessageType.Error;
        case "connected":
        case "safe":
            return Kirigami.MessageType.Positive;
        default:
            return Kirigami.MessageType.Information;
        }
    }
    actions: [
        Kirigami.Action {
            text: "Eject"
            icon.name: "media-eject"
            visible: message.backend.cardCanEject
            onTriggered: message.backend.eject()
        }
    ]
}
