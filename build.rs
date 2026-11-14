use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("dev.project516.clipplusmanager").qml_files([
        "qml/Main.qml",
        "qml/CardMessage.qml",
        "qml/CardState.qml",
        "qml/DownloadsPage.qml",
        "qml/DriveCard.qml",
        "qml/FindMusicPage.qml",
        "qml/JobDelegate.qml",
        "qml/NavItem.qml",
        "qml/PageSection.qml",
        "qml/PlayerPage.qml",
        "qml/RockboxChannelPicker.qml",
        "qml/RockboxInstallFlow.qml",
        "qml/RockboxManage.qml",
        "qml/RockboxPage.qml",
        "qml/SettingsPage.qml",
        "qml/ThemeDelegate.qml",
    ]))
    .files(["src/bridge.rs"])
    .build();
}
