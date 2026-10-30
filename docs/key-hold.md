# Give a key an optional two-second hold action

A physical keypad key can have a short action and a separate **Hold 2 seconds** action. They are exclusive: a short press runs the short action on release; a two-second hold runs only the secondary action. Keys without a hold binding still run immediately on key-down.

## Configure a key

1. Select a key and choose **Device > Start timer** from the Actions catalog. Set the duration and use **Save binding**.
2. Select **Hold 2 seconds** beside the action form, then choose **Device > Cancel timer** from the catalog. This parameterless action saves automatically; actions with settings need **Save binding**.
3. Release before two seconds to start the timer and show the time left on the key. Another short tap while it is running does nothing: it does not restart the countdown. Hold for at least two seconds to cancel it; releasing afterwards does not restart it. Start a new timer only after cancellation or completion.

The hold action is optional and can be any catalog action, not only cancellation. Selecting an action for the same key keeps the current short/hold mode. Opening or toggling the editor does not create or execute a binding. To edit the short action again, clear **Hold 2 seconds**.

**Remove binding** in hold mode removes only the secondary binding, restoring immediate key-down execution on the next press. Undo restores that binding and its page-return setting. An explicitly saved **None** secondary action is still a configured hold binding; remove it to disable hold recognition.

## What stays independent

- Hold actions do not replace the short action's artwork, button text or live-information binding. Cancelling a countdown restores the normal key image.
- Short and hold actions can each have their own page-return setting.
- This applies only to the eight physical keypad keys. Touch-strip hold gestures and dial presses keep their existing behavior.
- Swapping keys carries their short actions, hold actions and page-return settings together. Export/import retains both bindings without a configuration migration.

## Verification

Display-backed binary tests configure the timer and cancellation through the real Actions catalog and save commands, persist and reload the actual layer-store directory, then send press/release reports through the production input channel. They await GTK signals with a bounded timeout to verify short-tap countdown, two-second cancellation, artwork restoration and release suppression. Separate editor coverage checks save/reload/remove/undo and primary live-information preservation.

Library `key_hold` tests cover the two-second boundaries, duplicate snapshots, independent keys, captured actions/returns, cancellation, invalid input, swap/reload/unbind metadata and bundle round trips. Hardware behavior still requires a real device check; these tests do not access HID hardware.
