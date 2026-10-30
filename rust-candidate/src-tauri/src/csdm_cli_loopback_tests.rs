use super::*;
use std::net::{TcpListener,TcpStream};
use tungstenite::{client,Message,WebSocket};
fn wait_count(h:&Hub,n:usize){let until=Instant::now()+Duration::from_secs(3);while h.peer_count().unwrap()!=n {assert!(Instant::now()<until,"peer count did not reach {n}");thread::sleep(Duration::from_millis(10));}}
fn connect(addr:std::net::SocketAddr)->WebSocket<TcpStream>{let stream=TcpStream::connect_timeout(&addr,Duration::from_secs(2)).unwrap();stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();let (socket,_)=client(format!("ws://{addr}/?process=cli"),stream).unwrap();socket}
fn read_event(socket:&mut WebSocket<TcpStream>)->Value{loop{match socket.read().unwrap(){Message::Text(s)=>return serde_json::from_str(&s).unwrap(),Message::Ping(_)|Message::Pong(_)=>(),other=>panic!("unexpected event {other:?}")}}}
#[test]
#[ignore = "reviewed ephemeral loopback test requires action-specific authorization"]
fn pg106_live_multi_peer_settings_and_disconnect(){
 let listener=TcpListener::bind("127.0.0.1:0").unwrap();let addr=listener.local_addr().unwrap();assert!(addr.ip().is_loopback());
 let events=Arc::new(Mutex::new(Vec::<Value>::new()));let sink=events.clone();let hub=Arc::new(Hub::from_parts(Arc::new(Mutex::new(Analyses::new())),Arc::new(Mutex::new(())),|_,_|Err("unused".into()),||Ok(()),move|v|{sink.lock().unwrap().push(v);Ok(())}));let mut owner=CliListener::start(listener,hub.clone(),3).unwrap();
 let mut a=connect(addr);wait_count(&hub,1);let mut b=connect(addr);wait_count(&hub,2);let mut c=connect(addr);wait_count(&hub,3);
 let first=json!({"name":"settings-updated","payload":{"demos":{"tagIds":["1","2"]}}});hub.queue_settings(first.clone(),Duration::from_secs(3)).unwrap();assert_eq!(read_event(&mut a),first);assert_eq!(read_event(&mut b),first);assert_eq!(read_event(&mut c),first);assert_eq!(events.lock().unwrap().as_slice(),&[first]);
 a.close(None).unwrap();drop(a);wait_count(&hub,2);let second=json!({"name":"settings-updated","payload":{"demos":{"tagIds":["2"]}}});hub.queue_settings(second.clone(),Duration::from_secs(3)).unwrap();assert_eq!(read_event(&mut b),second);assert_eq!(read_event(&mut c),second);assert_eq!(events.lock().unwrap().len(),2);
 b.close(None).unwrap();drop(b);c.close(None).unwrap();drop(c);wait_count(&hub,0);owner.shutdown(Duration::from_secs(3)).unwrap();hub.shutdown_renderer(Duration::from_secs(3)).unwrap();assert!(hub.delivery_issues().unwrap().is_empty());println!("PG106_RUST_LOOPBACK ok first_peers=3 after_close=2 callbacks=2 peer_count=0 listener_stopped=true renderer_stopped=true port={}",addr.port());
}
