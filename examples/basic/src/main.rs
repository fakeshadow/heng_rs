use std::time::{Duration, Instant};

use heng_rs::{Scheduler, SendError, Time, ToDuration};
use tokio::time::delay_for;

// setup a new schedule
struct TestSchedule;

// impl Scheduler for our test schedule and infer message type
impl Scheduler for TestSchedule {
    type Message = u32;
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut now = Instant::now();

    // construct a scheduler
    let test = TestSchedule;

    // Time is a helper type for caculate duration.
    let time = Time::new().every(1.s());

    // start scheduler will return it's address.
    let addr = test
        // pass Time/Duration and a closure where you run the schedule and access the task self and context.
        .start(time, move |_task, ctx| {
            let new = Instant::now();
            println!("{:#?} have passed since last run", new.duration_since(now));
            now = new;

            //we can pop message using context.
            let msg = ctx.get_msg_front();

            // we can return a future with result in the closure
            async move {
                if let Some(msg) = msg {
                    println!("processing message: {}", msg);
                    /* do something with message*/

                    // if we take too long to finish.the scheduler will wait until the task at hand finish.
                    if msg == 32 {
                        delay_for(Duration::from_secs(1)).await;
                    }
                } else {
                    /* or something without message */
                }
                Ok::<(), ()>(())
            }
        });

    // use the address send message to scheduler.
    let _ = addr.send(32).await;

    // return error type is SendError from futures crate.
    let _r: Result<(), SendError> = addr.send(42).await;

    // signal the scheduler and temporary stop it.
    let _ = addr.stop().await;

    delay_for(Duration::from_secs(2)).await;

    // restart the scheduler.
    let _ = addr.start().await;

    // change the time of scheduler. This will drop the old scheduler and start a new one with the the new time given.
    let time = Time::new().every(500.millis());
    let _ = addr.change_time(time).await;

    delay_for(Duration::from_secs(2)).await;

    Ok(())
}
