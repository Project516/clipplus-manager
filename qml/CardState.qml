import QtQuick
import org.kde.kirigami as Kirigami

// Turns the engine's card kind into a label and a badge type.
QtObject {
    required property string kind

    readonly property bool connected: kind === "connected" || kind === "copying"
    readonly property bool away: kind === "none" || kind === "plug"
    readonly property string label: {
        switch (kind) {
        case "connected":
            return "Connected";
        case "copying":
            return "Copying songs";
        case "safe":
            return "Safe to unplug";
        case "busy":
            return "Ejecting";
        case "error":
            return "Problem";
        default:
            return "Not connected";
        }
    }
    readonly property int badgeType: {
        switch (kind) {
        case "connected":
        case "copying":
        case "safe":
            return Kirigami.Badge.Type.Positive;
        case "error":
            return Kirigami.Badge.Type.Error;
        default:
            return Kirigami.Badge.Type.Information;
        }
    }
}
