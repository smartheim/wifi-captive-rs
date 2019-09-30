var networks = undefined;
let selectBox = document.querySelector('#ssid-select');

function showHideEnterpriseSettings() {
    let security = selectBox.querySelector(':selected').dataset.security
    if(security === 'enterprise') {
        document.querySelector('#identity-group').classList.remove("hide")
    } else {
        document.querySelector('#identity-group').classList.add("hide")
    }
}

selectBox.addEventListener("input", showHideEnterpriseSettings);

async function get_networks() {
    let response = await fetch("/networks");
    if (!response.ok) {
        document.querySelector('.before-submit').classList.add("hide")
        document.querySelector('#no-networks-message').classList.remove("hide")
        return;
    }
    let networks = await response.json();
    for (let network in networks) {
        let option = document.createElement("option");
        option.innerHTML = network.ssid;
        option.value = network.ssid;
        option.dataset.security = network.security;
        selectBox.appendChild(option);
    }
    showHideEnterpriseSettings();
}

get_networks();

let form = document.querySelector('#connect-form');
form.addEventListener("submit", async ev => {
    const formData = new FormData(form);
    await fetch("/connect", {method: 'POST', body: formData})
        .catch(err => {});
    document.querySelector('.before-submit').classList.add('hide');
    document.querySelector('#submit-message').classList.remove('hide');
    ev.preventDefault();
});