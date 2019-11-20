# Required dbus crate changes

* Access to add_match, remove_match
* Process trait: Add "drops" method to resolve outstanding method-replies
* SyncConnection/Localconnection:
  Add `drop: Mutex<VecDeque<(String, MethodReply<()>)>>,`
  
Drops method on process:
```rust
fn drops(&self, ctx: &mut Context<'_>) {
    use std::future::Future;

    let mut drop = self.drop.lock().unwrap();
    let mut a = drop.drain(..).filter_map(|(match_str, mut method_reply)| {
        match unsafe { pin::Pin::new_unchecked(&mut method_reply) }.poll(ctx) {
            task::Poll::Pending => Some((match_str, method_reply)),
            task::Poll::Ready(_) => {
                info!("Drop stream complete - {}", match_str);
                None
            }
        }
    }).collect();
    drop.clear();
    drop.append(&mut a);
}
```