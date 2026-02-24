use std::sync::mpsc::SyncSender;

pub struct ClipboardTray {
    pub show_tx: SyncSender<()>,
    pub quit_tx: SyncSender<()>,
}

impl ksni::Tray for ClipboardTray {
    fn id(&self) -> String {
        "clipboard-manager".into()
    }

    fn icon_name(&self) -> String {
        "edit-paste".into()
    }

    fn title(&self) -> String {
        "Clipboard Manager".into()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            ksni::MenuItem::Standard(StandardItem {
                label: "Show History".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.show_tx.try_send(());
                }),
                ..Default::default()
            }),
            ksni::MenuItem::Separator,
            ksni::MenuItem::Standard(StandardItem {
                label: "Quit".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.quit_tx.try_send(());
                }),
                ..Default::default()
            }),
        ]
    }
}
