function prevent_interaction() {
    // Prevent right-click.
    document.addEventListener("contextmenu", event => event.preventDefault());
    // Prevent selection.
    document.addEventListener("mousedown", event => event.preventDefault());
    // Stop the cursor from changing over text.
    document.body.classList.add("fix_cursor");
}

function enable() {
    document.body.classList.remove("disabled");
    document.addEventListener("keydown", _keyboard_listener);
}

function disable() {
    document.removeEventListener("keydown", _keyboard_listener);
    document.body.classList.add("disabled");
}

function _keyboard_listener(event) {
    // Ignore irrelevant events.
    if (["Control", "Shift", "Alt", "Meta", "CapsLock", "Unidentified"].includes(event.key)) {
        return;
    }
    // Send the event to the Simulatron.
    let msg = {key: event.key, ctrl: event.ctrlKey, alt: event.altKey};
    window.webkit.messageHandlers.external.postMessage(JSON.stringify(msg));
    // Prevent Ctrl-A from selecting everything.
    if (event.ctrlKey && (event.key == "a" || event.key == "A")) {
        event.preventDefault();
    }
}

function set_char(row, col, char) {
    _get_cell(row, col).innerHTML = char;
}

function set_fg(row, col, colour) {
    _get_cell(row, col).style.color = colour;
}

function set_bg(row, col, colour) {
    _get_cell(row, col).style.background = colour;
}

function _get_cell(row, col) {
    return document.getElementById("screen").rows[row].cells[col];
}
