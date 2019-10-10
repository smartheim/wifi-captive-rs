// This script is late loaded and the dom is already established.
// For efficiency reasons the most common dom elements are global-variable bound.

const selectBox = document.getElementById('ssid-select');
const ssid_input = document.getElementById("ssid");
const passphrase_input = document.getElementById("passphrase");
const hw_input = document.getElementById("hw"); // wifi hw -> used as unique id
const submit_button = document.getElementById('submit_btn');

// Enable the submit button if a SSID (or ssid+password) is entered.
// The password must be optional to accommodate the case of an open wifi.
ssid_input.addEventListener("input", ev => {
    submit_button.disabled = ev.target.value.length === 0;
    unselect_entry();
});
passphrase_input.addEventListener("input", ev => {
    submit_button.disabled = ssid_input.value.length === 0 || passphrase_input.value.length === 0;
});

/**
 * Unselect wifi list entry. This also resets the hidden "hw" input,
 * which would otherwise uniquely identify the list entry to the backend.
 */
function unselect_entry() {
    document.querySelectorAll(".target_link").forEach(e => delete e.dataset.selected);
    hw_input.value = "";
}

/**
 * Callback for when an entry in the wifi list has been selected.
 *
 * @param selected {HTMLElement} The selected dom node
 * @param network {Object} The network data
 */
function entrySelected(selected, network) {
    if (network.security === 'enterprise') {
        submit_button.disabled = ssid_input.value.length === 0 || passphrase_input.value.length === 0;
        document.querySelector('#identity-group').classList.remove("hide");
        document.querySelector('#identity').classList.remove("hide");
    } else if (network.security === 'wpa' || network.security === 'wep') {
        submit_button.disabled = ssid_input.value.length === 0 || passphrase_input.value.length === 0;
        document.querySelector('#passphrase-group').classList.remove("hide");
        document.querySelector('#passphrase').classList.remove("hide");
    } else {
        submit_button.disabled = ssid_input.value.length === 0;
        document.querySelector('#identity-group').classList.add("hide");
        document.querySelector('#identity').classList.add("hide");
        document.querySelector('#passphrase-group').classList.add("hide");
        document.querySelector('#passphrase').classList.add("hide");
    }

    ssid_input.value = network.ssid;
    hw_input.value = network.hw;
    passphrase_input.focus();
}

/**
 * Creates a wifi entry and adds it to `selectBox`.
 *
 * @param id
 * @param network The network struct
 * @param network.strength {int} The strength of the network in percent
 * @param network.frequency {int} The frequency of the network in Mhz
 * @param network.ssid {string} The SSID
 * @param network.hw {string} The unique address (mac) of the wifi network
 * @param network.security {string} The security. May be "professional", "wpa", "wep", "open"
 */
function createOption(id, network) {
    let option = document.getElementById(id);
    let is_new = false;
    if (!option) {
        is_new = true;
        option = document.querySelector("#wifi_item").content.cloneNode(true).firstElementChild;
    }

    option.id = "ssid_" + id;

    const link = option.querySelector(".target_link");
    delete link.dataset.selected;
    link.addEventListener("click", ev => {
        ev.stopPropagation();
        ev.preventDefault();

        unselect_entry();
        link.dataset.selected = "true";
        entrySelected(option, network);
    });

    const strength = option.querySelector(".target_strength");
    strength.title = "Signal: " + network.strength + "%";
    strength.classList.add("waveStrength-" + Math.floor((network.strength + 10) * 4 / 100));

    const freq = network.frequency > 5000 ? "5 Ghz" : "2 Ghz";
    const label = option.querySelector(".target_name");
    label.innerHTML = network.ssid;

    const subtitle = option.querySelector(".target_subtitle");
    subtitle.innerHTML = "Signal: " + network.strength + "% - " + freq;

    const encrypted = option.querySelector(".encrypted");
    if (network.security !== "wpa" && network.security !== "enterprise" && network.security !== "wep")
        encrypted.classList.add("hide");

    if (is_new) selectBox.appendChild(option);
}

/**
 *
 * @returns {Promise<void>} Fulfills when the network refresh has been performed
 */
async function connection_reestablished() {
    let no_conn = document.getElementById("no_connection");
    if (!no_conn) return;

    no_conn.remove();
    document.querySelectorAll(".pure-button").forEach(e => e.classList.remove("pure-button-disabled"));
    await get_networks();
}

function connection_lost(error) {
    console.warn("SSE error", error);
    let no_conn = document.getElementById("no_connection");
    if (no_conn) return;

    while (selectBox.hasChildNodes()) {
        selectBox.removeChild(selectBox.lastChild);
    }

    no_conn = document.createElement("div");
    no_conn.id = "no_connection";
    no_conn.innerHTML = "<center>Connection lost</center>";
    selectBox.appendChild(no_conn);
    document.querySelectorAll(".pure-button").forEach(e => e.classList.add("pure-button-disabled"));
}

function receive_list_of_networks(networks) {
    networks.sort((b, a) => {
        if (a.strength < b.strength) return -1;
        else if (a.strength > b.strength) return 1;
        return 0;
    });
    for (let network of networks) {
        createOption("ssid_" + network.hw.replace(":", "_"), network);
    }
}

// Remove everything in the list so far, show the selection page and refresh the network list.
// Networks are sorted by signal strength
async function get_networks() {
    while (selectBox.hasChildNodes()) {
        selectBox.removeChild(selectBox.lastChild);
    }

    document.querySelector('#choose_wifi').classList.remove('hide');

    let response = await fetch("/networks");
    if (!response.ok) {
        document.querySelector('#connect-error').classList.remove("hide");
        return;
    }
    receive_list_of_networks(await response.json());
}

get_networks()
    .then(() => {
        // There are three types of events coming form the backend: Added, Removed, List
        const evtSource = new EventSource("/events");

        evtSource.addEventListener("List", async event => {
            await connection_reestablished();
            receive_list_of_networks(JSON.parse(event.data));
        });

        evtSource.addEventListener("Added", async event => {
            await connection_reestablished();

            let event_data = JSON.parse(event.data);
            let id = "ssid_" + event_data.hw.replace(":", "_");
            console.log("Wifi added/updated", event_data);
            createOption(id, event_data)
        });

        evtSource.addEventListener("Removed", async event => {
            await connection_reestablished();

            let event_data = JSON.parse(event.data);
            let el = document.querySelector("#ssid_" + event_data.hw.replace(":", "_"));
            if (el) el.remove();
            console.log("Wifi removed", event_data);
        });
        // Display an error message if connection lost
        evtSource.onerror = connection_lost;

        window.addEventListener('offline', () => {
            document.getElementById("content-offline").classList.remove("hide");
        });
        window.addEventListener('online', function (e) {
            document.getElementById("content-offline").classList.add("hide");
        });
    })
    .catch(e => console.error("Failed to fetch", e));

/**
 * Handle the refresh button
 *
 * This is a little bit of a fake for the user to "see" something.
 *
 * 1. We erase the network list although that is strictly speaking not necessary.
 * 2. We then instruct the backend to scan for wifis (which will take a few seconds, but
 *    server-send-events will call us back).
 * 3. After 500ms we re-request the wifi network list. The scan is probably not yet done, but the
 *    we will show the backends cached version. As already mentioned, a brand new list
 *    is propagated via server-send-events.
 */
function handle_refresh_button(ev) {
    ev.preventDefault();
    ev.stopPropagation();

    while (selectBox.hasChildNodes()) {
        selectBox.removeChild(selectBox.lastChild);
    }

    fetch("/refresh").catch(err => {
        document.querySelector('#connect-error').classList.remove('hide');
        console.log("Failed to submit", err);
    });

    setTimeout(get_networks, 500);
}

document.getElementById("refresh_button").addEventListener("click", handle_refresh_button);

/// Handle the form submit
let form = document.querySelector('form');
form.addEventListener("submit", ev => {
    ev.preventDefault();
    ev.stopPropagation();
    document.querySelector('#choose_wifi').classList.add('hide');
    document.querySelector('#connect-error').classList.add('hide');
    document.querySelector('#applying').classList.remove('hide');

    const formData = new FormData(form);
    const object = {};
    formData.forEach((value, key) => {
        if (value && value.length) object[key] = value
    });
    const json = JSON.stringify(object);

    fetch("/connect", {method: 'POST', body: json}).then(() => {
    }).catch(err => {
        document.querySelector('#applying').classList.add('hide');
        get_networks().catch(e => console.error("Failed to fetch", e));
        document.querySelector('#connect-error').classList.remove('hide');
        console.log("Failed to submit", err);
    });
});