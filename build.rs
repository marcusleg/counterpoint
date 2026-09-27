use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("Counterpoint").qml_files([
        "qml/Main.qml",
        "qml/ChatPane.qml",
        "qml/ProposalCard.qml",
        "qml/SettingsDialog.qml",
    ]))
    // Links QtQuick so QQuickTextDocument resolves, also for `cargo test`.
    .qt_module("Quick")
    .include_dir("cpp")
    .cpp_files([
        "cpp/markdown_highlighter.cpp",
        "cpp/plaintext.cpp",
        "cpp/textdoc.cpp",
    ])
    .files([
        "src/bridge/chat_controller.rs",
        "src/bridge/document_controller.rs",
        "src/bridge/settings_controller.rs",
    ])
    .build();
}
