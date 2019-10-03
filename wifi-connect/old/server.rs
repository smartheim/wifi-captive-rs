use std::error::Error as StdError;
use std::fmt;
use std::net::Ipv4Addr;
use std::sync::mpsc::{Receiver, Sender};

use iron::modifiers::Redirect;
use iron::prelude::*;
use iron::{
    headers, status, typemap, AfterMiddleware, Iron, IronError, IronResult, Request, Response, Url,
};
use iron_cors::CorsMiddleware;
use mount::Mount;
use params::{FromValue, Params};
use persistent::Write;
use router::Router;
use serde_json;
use staticfile::Static;
use std::path::PathBuf;

fn networks(req: &mut Request) -> IronResult<Response> {
    info!("User connected to the captive portal");

    let request_state = get_request_state!(req);

    if let Err(_e) = request_state.network_tx.send(StateMachine::ActivatePortal) {
        return Err(IronError::new(
            StringError("SendNetworkCommandActivate".to_owned()),
            status::InternalServerError,
        ));
    }

    let networks = match request_state.server_rx.recv() {
        Ok(result) => match result {
            NetworkCommandResponse::Networks(networks) => networks,
        },
        Err(_e) => {
            return Err(IronError::new(
                StringError("RecvAccessPointSSIDs".to_owned()),
                status::InternalServerError,
            ));
        }
    };

    let access_points_json = match serde_json::to_string(&networks) {
        Ok(json) => json,
        Err(_e) => {
            return Err(IronError::new(
                StringError("SerializeAccessPointSSIDs".to_owned()),
                status::InternalServerError,
            ));
        }
    };

    Ok(Response::with((status::Ok, access_points_json)))
}

fn connect(req: &mut Request) -> IronResult<Response> {
    let (ssid, identity, passphrase) = {
        let params = get_request_ref!(req, Params, "Getting request params failed");
        let ssid = get_param!(params, "ssid", String);
        let identity = get_param!(params, "identity", String);
        let passphrase = get_param!(params, "passphrase", String);
        (ssid, identity, passphrase)
    };

    debug!("Incoming `connect` to access point `{}` request", ssid);

    let request_state = get_request_state!(req);

    let command = StateMachine::ReconnectAttempt {
        ssid: ssid,
        identity: identity,
        passphrase: passphrase,
    };

    if let Err(_e) = request_state.network_tx.send(command) {
        Err(IronError::new(
            StringError("SendNetworkCommandConnect".to_owned()),
            status::InternalServerError,
        ))
    } else {
        Ok(Response::with(status::Ok))
    }
}
