
mod types;


use std::{
    io::{self, Write},
    str::FromStr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ldk_node::{bitcoin, bitcoin::{secp256k1::PublicKey, Network}, config::ChannelConfig, lightning::{
    ln::msgs::SocketAddress,
    offers::offer::Offer,
}, lightning_invoice::Bolt11Invoice, payment::SendingParameters, Builder, ChannelDetails, Node};

use lightning::ln::types::ChannelId;
use ureq::Agent;
use types::{Bitcoin};

fn make_node(alias: &str, port: u16, lsp_pubkey: Option<PublicKey>) -> ldk_node::Node {
    let mut builder = Builder::new();

    // If we pass in an LSP pubkey then set your liquidity source
    if let Some(lsp_pubkey) = lsp_pubkey {
        println!("{}", lsp_pubkey.to_string());
        let address = "127.0.0.1:9377".parse().unwrap();
        builder.set_liquidity_source_lsps2(
            address,
            lsp_pubkey,
            Some("00000000000000000000000000000000".to_owned()),
        );
    }

    builder.set_network(Network::Signet);

    // If this doesn't work, try the other one
    builder.set_chain_source_esplora("https://mutinynet.com/api/".to_string(), None);
    // builder.set_esplora_server("https://mutinynet.ltbl.io/api".to_string());

    // Don't need gossip right now. Also interferes with Bolt12 implementation.
    // builder.set_gossip_source_rgs("https://mutinynet.ltbl.io/snapshot".to_string());
    builder.set_storage_dir_path(("./data/".to_owned() + alias).to_string());
    let _ = builder.set_listening_addresses(vec![format!("127.0.0.1:{}", port).parse().unwrap()]);
    let _ = builder.set_node_alias("some_alias".to_string()); // needed to open announced channel since LDK 0.4.0

    let node = builder.build().unwrap();
    node.start().unwrap();
    let public_key: PublicKey = node.node_id();

    let listening_addresses: Vec<SocketAddress> = node.listening_addresses().unwrap();

    if let Some(first_address) = listening_addresses.first() {
        println!("");
        println!("Actor Role: {}", alias);
        println!("Public Key: {}", public_key);
        println!("Internet Address: {}", first_address);
        println!("");
    } else {
        println!("No listening addresses found.");
    }

    return node;
}

fn get_user_input(prompt: &str) -> (String, Option<String>, Vec<String>) {
    let mut input = String::new();
    print!("{}", prompt);
    io::stdout().flush().unwrap();
    io::stdin().read_line(&mut input).unwrap();

    let input = input.trim().to_string();

    let mut parts = input.split_whitespace();
    let command = parts.next().map(|s| s.to_string());
    let args: Vec<String> = parts.map(|s| s.to_string()).collect();

    (input, command, args)
}
/// Program initialization and command-line-interface
fn main() {


    #[cfg(feature = "user")]
    {
        let user = make_node("user", 9736, None);
        let mut their_offer: Option<Offer> = None;

        loop {
            let (_input, command, args) = get_user_input("Enter command for user: ");

            match (command.as_deref(), args.as_slice()) {
                (Some("onchaintransfer"), args) => {
                    if args.len() != 2 {
                        println!("Error: 'onchaintransfer' command requires two parameters: <destination_address> and <sats>");
                        return;
                    }

                    let destination_address = match bitcoin::address::Address::from_str(&args[0]) {
                        Ok(addr) => {addr},
                        Err(_) => {
                            println!("Invalid bitcoin address");
                            return;
                        }
                    };

                    let destination_address = destination_address.require_network(bitcoin::Network::Signet).unwrap();

                    let sats_str = &args[1];

                    match sats_str.parse::<u64>() {
                        Ok(sats) => {
                            match user.onchain_payment().send_to_address(&destination_address, sats) {
                                Ok(txid) => println!("On-chain transfer successful. Transaction ID: {}", txid),
                                Err(e) => println!("Error sending on-chain transfer: {}", e),
                            }
                        },
                        Err(_) => println!("Invalid amount of satoshis provided"),
                    }
                }

                (Some("getaddress"), []) => {
                    let funding_address = user.onchain_payment().new_address();
                    match funding_address {
                        Ok(fund_addr) => println!("User Funding Address: {}", fund_addr),
                        Err(e) => println!("Error getting funding address: {}", e),
                    }
                }
                (Some("openchannel"), args) => {
                    if args.len() != 3 {
                        println!("Error: 'openchannel' command requires three parameters: <node_id>, <listening_address>, and <sats>");
                        return;
                    }

                    // TODO - set zero reserve
                    // ChannelHandshakeConfig::their_channel_reserve_proportional_millionths
                    // https://docs.rs/lightning/latest/lightning/util/config/struct.ChannelHandshakeConfig.html#structfield.their_channel_reserve_proportional_millionths

                    // https://docs.rs/lightning/latest/lightning/util/config/struct.ChannelHandshakeLimits.html#structfield.max_channel_reserve_satoshis

                    let node_id_str = &args[0];
                    let listening_address_str = &args[1];
                    let sats_str = &args[2];

                    let lsp_node_id = node_id_str.parse().unwrap();
                    let lsp_net_address: SocketAddress = listening_address_str.parse().unwrap();
                    let sats: u64 = sats_str.parse().unwrap();
                    let push_msat = (sats / 2) * 1000;

                    let channel_config: Option<ChannelConfig> = None;

                    match user.open_announced_channel(
                        lsp_node_id,
                        lsp_net_address,
                        sats,
                        Some(push_msat),
                        channel_config,
                    ) {
                        Ok(_) => println!("Channel successfully opened to {}", node_id_str),
                        Err(e) => println!("Failed to open channel: {}", e),
                    }
                }
                (Some("balance"), []) => {
                    let balances = user.list_balances();
                    let onchain_balance = Bitcoin::from_sats(balances.total_onchain_balance_sats);
                    let lightning_balance =
                        Bitcoin::from_sats(balances.total_lightning_balance_sats);
                    println!("User On-Chain Balance: {}", onchain_balance);
                    println!("Stable Receiver Lightning Balance: {}", lightning_balance);
                }
                (Some("connecttolsp"), []) => {}
                (Some("closeallchannels"), []) => {
                    for channel in user.list_channels().iter() {
                        let user_channel_id = channel.user_channel_id;
                        let counterparty_node_id = channel.counterparty_node_id;
                        let _ = user.close_channel(&user_channel_id, counterparty_node_id);
                    }
                    print!("Closing all channels.")
                }
                (Some("listallchannels"), []) => {
                    let channels = user.list_channels();
                    if channels.is_empty() {
                        println!("No channels found.");
                    } else {
                        println!("User Channels:");
                        for channel in channels.iter() {
                            println!("--------------------------------------------");
                            println!("Channel ID: {}", channel.channel_id);
                            println!(
                                "Channel Value: {}",
                                Bitcoin::from_sats(channel.channel_value_sats)
                            );
                            // println!("Our Balance: {}", Bitcoin::from_sats(channel.outbound_capacity_msat / 1000));
                            // println!("Their Balance: {}", Bitcoin::from_sats(channel.inbound_capacity_msat / 1000));
                            println!("Channel Ready?: {}", channel.is_channel_ready);
                        }
                        println!("--------------------------------------------");
                    }
                }
                (Some("getinvoice"), [sats]) => {
                    if let Ok(sats_value) = sats.parse::<u64>() {
                        let msats = sats_value * 1000;
                        let bolt11: ldk_node::payment::Bolt11Payment = user.bolt11_payment();
                        let invoice = bolt11.receive(msats, "test invoice", 6000);
                        match invoice {
                            Ok(inv) => println!("User Invoice: {}", inv),
                            Err(e) => println!("Error creating invoice: {}", e),
                        }
                    } else {
                        println!("Invalid sats value provided");
                    }
                }
                (Some("payinvoice"), [invoice_str]) => {
                    let bolt11_invoice = invoice_str.parse::<Bolt11Invoice>();
                    match bolt11_invoice {
                        Ok(invoice) => match user.bolt11_payment().send(&invoice, None) {
                            Ok(payment_id) => {
                                println!("Payment sent from User with payment_id: {}", payment_id)
                            }
                            Err(e) => println!("Error sending payment from User: {}", e),
                        },
                        Err(e) => println!("Error parsing invoice: {}", e),
                    }
                }
                (Some("getjitinvoice"), []) => {
                    match user.bolt11_payment().receive_via_jit_channel(
                        50000000,
                        "Stable Channel",
                        3600,
                        Some(10000000),
                    ) {
                        Ok(invoice) => println!("Invoice: {:?}", invoice.to_string()),
                        Err(e) => println!("Error: {:?}", e),
                    }
                }
                (Some("exit"), _) => break,
                _ => println!("Unknown command or incorrect arguments:"),
            }
        }
    }

    #[cfg(feature = "lsp")]
    {
        let lsp = make_node("lsp", 9737, None);
        let mut their_offer: Option<Offer> = None;

        loop {
            let (input, command, args) = get_user_input("Enter command for lsp: ");

            match (command.as_deref(), args.as_slice()) {

                (Some("onchaintransfer"), args) => {
                    if args.len() != 2 {
                        println!("Error: 'onchaintransfer' command requires two parameters: <destination_address> and <sats>");
                        return;
                    }

                    let destination_address = match bitcoin::address::Address::from_str(&args[0]) {
                        Ok(addr) => {addr},
                        Err(_) => {
                            println!("Invalid bitcoin address");
                            return;
                        }
                    };

                    let destination_address = destination_address.require_network(bitcoin::Network::Signet).unwrap();

                    let sats_str = &args[1];

                    match sats_str.parse::<u64>() {
                        Ok(sats) => {
                            match lsp.onchain_payment().send_to_address(&destination_address, sats) {
                                Ok(txid) => println!("On-chain transfer successful. Transaction ID: {}", txid),
                                Err(e) => println!("Error sending on-chain transfer: {}", e),
                            }
                        },
                        Err(_) => println!("Invalid amount of satoshis provided"),
                    }
                }

                (Some("getaddress"), []) => {
                    let funding_address = lsp.onchain_payment().new_address();
                    match funding_address {
                        Ok(fund_addr) => println!("LSP Funding Address: {}", fund_addr),
                        Err(e) => println!("Error getting funding address: {}", e),
                    }
                }
                (Some("openchannel"), args) => {
                    if args.len() != 3 {
                        println!("Error: 'openchannel' command requires three parameters: <node_id>, <listening_address>, and <sats>");
                        return;
                    }

                    let node_id_str = &args[0];
                    let listening_address_str = &args[1];
                    let sats_str = &args[2];

                    let user_node_id = node_id_str.parse().unwrap();
                    let lsp_net_address: SocketAddress = listening_address_str.parse().unwrap();
                    let sats: u64 = sats_str.parse().unwrap();

                    let channel_config: Option<ChannelConfig> = None;

                    match lsp.open_announced_channel(
                        user_node_id,
                        lsp_net_address,
                        sats,
                        Some(sats / 2),
                        channel_config,
                    ) {
                        Ok(_) => println!("Channel successfully opened to {}", node_id_str),
                        Err(e) => println!("Failed to open channel: {}", e),
                    }
                }

                (Some("balance"), []) => {
                    let balances = lsp.list_balances();
                    let onchain_balance = Bitcoin::from_sats(balances.total_onchain_balance_sats);
                    let lightning_balance =
                        Bitcoin::from_sats(balances.total_lightning_balance_sats);
                    println!("LSP On-Chain Balance: {}", onchain_balance);
                    println!("LSP Lightning Balance: {}", lightning_balance);
                }
                (Some("listallchannels"), []) => {
                    println!("channels:");
                    for channel in lsp.list_channels().iter() {
                        let channel_id = channel.channel_id;
                        println!("{}", channel_id);
                    }
                    println!("channel details:");
                    let channels = lsp.list_channels();
                    println!("{:#?}", channels);
                }
                (Some("getinvoice"), [sats]) => {
                    if let Ok(sats_value) = sats.parse::<u64>() {
                        let msats = sats_value * 1000;
                        let bolt11 = lsp.bolt11_payment();
                        let invoice = bolt11.receive(msats, "test invoice", 6000);
                        match invoice {
                            Ok(inv) => println!("LSP Invoice: {}", inv),
                            Err(e) => println!("Error creating invoice: {}", e),
                        }
                    } else {
                        println!("Invalid sats value provided");
                    }
                }
                (Some("payinvoice"), [invoice_str]) => {
                    let bolt11_invoice = invoice_str.parse::<Bolt11Invoice>();
                    match bolt11_invoice {
                        Ok(invoice) => match lsp.bolt11_payment().send(&invoice, None) {
                            Ok(payment_id) => {
                                println!("Payment sent from LSP with payment_id: {}", payment_id)
                            }
                            Err(e) => println!("Error sending payment from LSP: {}", e),
                        },
                        Err(e) => println!("Error parsing invoice: {}", e),
                    }
                }
                (Some("exit"), _) => break,
                _ => println!("Unknown command or incorrect arguments: {}", input),
            }
        }
    }
}

