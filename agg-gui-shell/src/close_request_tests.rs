//! Port of agg-sharp `Tests/Agg.Tests/Agg.UI/PlatformCloseRoutingTests.cs`
//! (`TheShellClosesEvenWhileADialogIsOnTop`,
//! `AShellThatVetoesKeepsItsNativeWindow`,
//! `AVetoOnTheSecondAskPutsTheHostBack`) over `close_request.rs`. C#'s
//! `SystemWindow { IsApplicationShell = true }` with a `ShouldClose` handler
//! is a `ShellHost` whose `on_close_requested` answers; the dialog on top is
//! a modal `Window` in the App's tree. The class's other four tests pin C#'s
//! `SingleWindowProvider` routing, which has no Rust counterpart.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widgets::{FlexColumn, Window};
use agg_gui::{App, Widget};

use super::close_requested;
use crate::ShellHost;

/// The application shell: counts its asks and answers with `allow`.
struct Shell {
    asks: Rc<Cell<u32>>,
    allow: Box<dyn Fn(u32) -> bool>,
}

impl ShellHost for Shell {
    fn on_close_requested(&mut self, _app: &mut App) -> bool {
        self.asks.set(self.asks.get() + 1);
        (self.allow)(self.asks.get())
    }
}

fn shell(allow: impl Fn(u32) -> bool + 'static) -> (Shell, Rc<Cell<u32>>) {
    let asks = Rc::new(Cell::new(0));
    let host = Shell {
        asks: Rc::clone(&asks),
        allow: Box::new(allow),
    };
    (host, asks)
}

/// An App with a modal dialog on top, and whether the dialog was closed.
fn app_with_dialog() -> (App, Rc<Cell<bool>>) {
    let dialog_closed = Rc::new(Cell::new(false));
    let closed = Rc::clone(&dialog_closed);
    let font = Arc::new(agg_gui::fonts::standard_ui_font());
    let dialog = Window::new("Dialog", font, Box::new(FlexColumn::new()))
        .with_modal(true)
        .on_close(move |_| closed.set(true));
    let mut root = FlexColumn::new();
    root.children_mut().push(Box::new(dialog));
    (App::new(Box::new(root)), dialog_closed)
}

#[test]
fn the_shell_closes_even_while_a_dialog_is_on_top() {
    let (mut host, asks) = shell(|_| true);
    let (mut app, dialog_closed) = app_with_dialog();

    let may_close = close_requested(&mut host, &mut app);

    assert!(may_close);
    assert_eq!(asks.get(), 1, "the shell's own close handler ran");
    assert!(
        !dialog_closed.get(),
        "the close is about the application, not the dialog"
    );
}

#[test]
fn a_shell_that_vetoes_keeps_its_native_window() {
    // What MatterCAD does on the first ask: cancel, persist the open tabs,
    // then close itself on idle.
    let (mut host, asks) = shell(|_| false);
    let (mut app, dialog_closed) = app_with_dialog();

    let may_close = close_requested(&mut host, &mut app);

    assert!(!may_close);
    assert_eq!(asks.get(), 1);
    assert!(!dialog_closed.get());
}

#[test]
fn a_veto_on_the_second_ask_puts_the_host_back() {
    // C#'s GuiWidget.Close asks again, and an application that opened a
    // "save first?" dialog on the first ask cancels on the second, which left
    // the host believing it was mid-close. Here each native request asks the
    // shell exactly once and acts on that answer, so a second ask never
    // overrules a first one the platform already acted on, and a veto on any
    // later request keeps the window: there is no mid-close state.
    let (mut host, asks) = shell(|asks| asks <= 1);
    let (mut app, _) = app_with_dialog();

    assert!(close_requested(&mut host, &mut app));
    assert_eq!(asks.get(), 1, "one native request is one ask");

    assert!(!close_requested(&mut host, &mut app));
    assert_eq!(asks.get(), 2);
}
