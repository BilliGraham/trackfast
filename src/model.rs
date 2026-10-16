#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Timer,
    Projects,
}

#[derive(Default)]
pub struct State {
    pub screen: Screen,
    pub widget_open: bool,
}

pub enum Action {
    Navigate(Screen),
    OpenWidget,
    CloseWidget,
}
