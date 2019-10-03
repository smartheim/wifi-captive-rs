var networks = undefined;
let selectBox = document.querySelector('#ssid-select');

function showHideEnterpriseSettings() {
    let security = selectBox.querySelector(':checked').dataset.security;

    if(security === 'enterprise') {
        document.querySelector('#identity-group').classList.remove("hide");
        document.querySelector('#identity').classList.remove("hide");
    } else {
        document.querySelector('#identity-group').classList.add("hide");
        document.querySelector('#identity').classList.add("hide");
    }

     console.log("SEC", security, document.querySelector('#identity').classList);
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
    for (let network of networks) {
        let option = document.createElement("option");
        option.innerHTML = "(Signal: "+network.strength+"%) "+network.ssid;
        option.value = network.ssid; // TODO uuid
        option.dataset.security = network.security;
        selectBox.appendChild(option);
    }
    showHideEnterpriseSettings();
}

get_networks();

let form = document.querySelector('#connect-form');
form.addEventListener("submit", ev => {
    ev.preventDefault();
    ev.stopPropagation();
    const formData = new FormData(form);
    var object = {};
    formData.forEach((value, key) => {object[key] = value});
    var json = JSON.stringify(object);

    fetch("/connect", {method: 'POST', body: json}).then(()=> {
        document.querySelector('#submit-error').classList.add('hide');
        document.querySelector('#submit-message').classList.remove('hide');
    }).catch(err => {
        document.querySelector('#submit-error').classList.remove('hide');
        document.querySelector('#submit-message').classList.add('hide');
        console.log("Failed to submit", err);
    });
});