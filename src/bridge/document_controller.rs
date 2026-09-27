//! QObject that loads, saves and edits the editor's Markdown source text.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;
    }

    unsafe extern "C++Qt" {
        include!(<QtQuick/QQuickTextDocument>);
        #[qobject]
        type QQuickTextDocument;
    }

    unsafe extern "C++" {
        include!("textdoc.h");
        #[cxx_name = "textdocText"]
        fn textdoc_text(doc: &QQuickTextDocument) -> QString;
        #[cxx_name = "textdocLoadText"]
        fn textdoc_load_text(doc: Pin<&mut QQuickTextDocument>, text: &QString);
        #[cxx_name = "textdocRangeText"]
        fn textdoc_range_text(doc: &QQuickTextDocument, start: i32, end: i32) -> QString;
        #[cxx_name = "textdocReplaceTextUndoable"]
        fn textdoc_replace_text_undoable(doc: Pin<&mut QQuickTextDocument>, text: &QString);
        #[cxx_name = "textdocSetModified"]
        fn textdoc_set_modified(doc: Pin<&mut QQuickTextDocument>, modified: bool);
        #[cxx_name = "textdocAttachHighlighter"]
        fn textdoc_attach_highlighter(doc: Pin<&mut QQuickTextDocument>);
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, file_path, cxx_name = "filePath")]
        #[qproperty(QString, file_name, cxx_name = "fileName")]
        type DocumentController = super::DocumentControllerRust;

        #[qinvokable]
        unsafe fn open(self: Pin<&mut Self>, doc: *mut QQuickTextDocument, url: &QUrl) -> bool;

        #[qinvokable]
        unsafe fn save(self: Pin<&mut Self>, doc: *mut QQuickTextDocument) -> bool;

        #[qinvokable]
        #[cxx_name = "saveAs"]
        unsafe fn save_as(self: Pin<&mut Self>, doc: *mut QQuickTextDocument, url: &QUrl) -> bool;

        #[qinvokable]
        unsafe fn markdown(self: &Self, doc: *mut QQuickTextDocument) -> QString;

        #[qinvokable]
        #[cxx_name = "selectionMarkdown"]
        unsafe fn selection_markdown(
            self: &Self,
            doc: *mut QQuickTextDocument,
            start: i32,
            end: i32,
        ) -> QString;

        #[qinvokable]
        #[cxx_name = "setMarkdownUndoable"]
        unsafe fn set_markdown_undoable(
            self: Pin<&mut Self>,
            doc: *mut QQuickTextDocument,
            markdown: &QString,
        );

        #[qinvokable]
        #[cxx_name = "attachHighlighter"]
        unsafe fn attach_highlighter(self: &Self, doc: *mut QQuickTextDocument);

        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut Self>, message: QString);
    }
}

use core::pin::Pin;
use std::path::{Path, PathBuf};

use cxx_qt::CxxQtType;
use cxx_qt_lib::{QString, QUrl};

use crate::document;

#[derive(Default)]
pub struct DocumentControllerRust {
    file_path: QString,
    file_name: QString,
    /// True if the currently open file uses CRLF line endings, so a save converts back to CRLF.
    /// A newly created (never opened) document stays LF.
    crlf: bool,
}

impl qobject::DocumentController {
    unsafe fn open(
        mut self: Pin<&mut Self>,
        doc: *mut qobject::QQuickTextDocument,
        url: &QUrl,
    ) -> bool {
        let (Some(doc), Some(path)) = (doc.as_mut(), local_path(url)) else {
            return false;
        };
        match document::read_file(&path) {
            Ok(markdown) => {
                self.as_mut().rust_mut().crlf = document::uses_crlf(&markdown);
                qobject::textdoc_load_text(
                    Pin::new_unchecked(doc),
                    &QString::from(markdown.as_str()),
                );
                self.as_mut().set_current_file(&path);
                true
            }
            Err(message) => {
                self.error_occurred(QString::from(message.as_str()));
                false
            }
        }
    }

    unsafe fn save(self: Pin<&mut Self>, doc: *mut qobject::QQuickTextDocument) -> bool {
        let path = self.file_path().to_string();
        if path.is_empty() {
            return false;
        }
        self.write_to(doc, PathBuf::from(path))
    }

    unsafe fn save_as(
        self: Pin<&mut Self>,
        doc: *mut qobject::QQuickTextDocument,
        url: &QUrl,
    ) -> bool {
        match local_path(url) {
            Some(path) => self.write_to(doc, path),
            None => false,
        }
    }

    unsafe fn markdown(&self, doc: *mut qobject::QQuickTextDocument) -> QString {
        doc.as_ref().map(qobject::textdoc_text).unwrap_or_default()
    }

    unsafe fn selection_markdown(
        &self,
        doc: *mut qobject::QQuickTextDocument,
        start: i32,
        end: i32,
    ) -> QString {
        doc.as_ref()
            .map(|doc| qobject::textdoc_range_text(doc, start, end))
            .unwrap_or_default()
    }

    unsafe fn set_markdown_undoable(
        self: Pin<&mut Self>,
        doc: *mut qobject::QQuickTextDocument,
        markdown: &QString,
    ) {
        if let Some(doc) = doc.as_mut() {
            qobject::textdoc_replace_text_undoable(Pin::new_unchecked(doc), markdown);
        }
    }

    unsafe fn attach_highlighter(&self, doc: *mut qobject::QQuickTextDocument) {
        if let Some(doc) = doc.as_mut() {
            qobject::textdoc_attach_highlighter(Pin::new_unchecked(doc));
        }
    }

    unsafe fn write_to(
        mut self: Pin<&mut Self>,
        doc: *mut qobject::QQuickTextDocument,
        path: PathBuf,
    ) -> bool {
        let Some(doc) = doc.as_mut() else {
            return false;
        };
        let markdown = qobject::textdoc_text(doc).to_string();
        let markdown = if self.rust().crlf {
            document::to_crlf(&markdown)
        } else {
            markdown
        };
        match document::write_file(&path, &markdown) {
            Ok(()) => {
                qobject::textdoc_set_modified(Pin::new_unchecked(doc), false);
                self.as_mut().set_current_file(&path);
                true
            }
            Err(message) => {
                self.error_occurred(QString::from(message.as_str()));
                false
            }
        }
    }

    fn set_current_file(mut self: Pin<&mut Self>, path: &Path) {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.as_mut()
            .set_file_path(QString::from(&*path.to_string_lossy()));
        self.as_mut().set_file_name(QString::from(name.as_str()));
    }
}

fn local_path(url: &QUrl) -> Option<PathBuf> {
    url.to_local_file()
        .map(|path| PathBuf::from(path.to_string()))
        .filter(|path| !path.as_os_str().is_empty())
}
