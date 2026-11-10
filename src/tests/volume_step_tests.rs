use super::*;

#[test]
fn volume_step_field_follows_the_selected_action_and_validates_input() {
    run_gtk_test(|| {
        if !init_gtk_for_test() {
            return;
        }
        let window = gtk::Window::new();
        let editor = feature_editor::FeatureEditor::new();
        window.set_child(Some(&editor.root));
        window.present();
        wait_for_frame(&window);

        editor.assert_volume_step_field();
        editor.assert_roundtrips();
    });
}
