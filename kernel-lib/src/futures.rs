use core::future::Future;

#[doc(hidden)]
pub fn dummy_waker() -> core::task::Waker {
    ::dummy_waker::dummy_waker()
}
#[macro_export]
macro_rules! await_sync {
    ($e:expr) => {{
        use core::future::Future;
        let mut pinned_future = ::core::pin::pin!($e);
        let waker = $crate::futures::dummy_waker();
        let context = &mut ::core::task::Context::from_waker(&waker);
        loop {
            match pinned_future.as_mut().poll(context) {
                ::core::task::Poll::Ready(value) => break value,
                _ => continue,
            }
        }
    }};
}

#[macro_export]
macro_rules! await_once_noblocking {
    ($e:expr) => {{
        use core::future::Future;
        let mut pinned_future = ::core::pin::pin!($e);
        let waker = $crate::futures::dummy_waker();
        let context = &mut ::core::task::Context::from_waker(&waker);
        match pinned_future.as_mut().poll(context) {
            ::core::task::Poll::Ready(value) => Some(value),
            ::core::task::Poll::Pending => None,
        }
    }};
}

pub struct PendingOnceFuture {
    polled: bool,
}

impl PendingOnceFuture {
    pub fn new() -> Self {
        Self { polled: false }
    }
}

impl Future for PendingOnceFuture {
    type Output = ();

    fn poll(mut self: core::pin::Pin<&mut Self>, _: &mut core::task::Context) -> core::task::Poll<()> {
        if self.polled {
            core::task::Poll::Ready(())
        } else {
            self.polled = true;
            core::task::Poll::Pending
        }
    }
}

pub fn yield_pending() -> impl Future<Output = ()> {
    PendingOnceFuture::new()
}


#[cfg(test)]
mod tests {
    use core::future::Future;
    #[test]
    fn just_await() {
        async fn return_1() -> u32 {
            1
        }

        assert_eq!(await_sync!(return_1()), 1);
    }

    #[test]
    fn randomly_await() {
        async fn summation(until: usize) -> usize {
            let mut sum = 0;
            if until % 2 == 0 {
                return 0;
            }
            for i in 0..until {
                sum += RandomFuture::new(i).await;
            }
            sum
        }

        async fn summation2() -> usize {
            let mut sum = 0;
            for i in 0..10 {
                sum += summation(i).await;
            }
            sum
        }
        assert_eq!(await_sync!(summation(11)), 55);
        assert_eq!(await_sync!(summation(10)), 0);
        assert_eq!(await_sync!(summation2()), 70);
    }

    struct RandomFuture {
        value: usize,
    }

    impl RandomFuture {
        fn new(value: usize) -> Self {
            Self { value }
        }
    }

    impl Future for RandomFuture {
        type Output = usize;
        fn poll(
            self: core::pin::Pin<&mut Self>,
            _: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Self::Output> {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            if rng.gen_bool(0.5) {
                core::task::Poll::Pending
            } else {
                core::task::Poll::Ready(self.value)
            }
        }
    }

    #[derive(Debug, PartialEq)]
    struct TimesPendingFuture<T: Unpin + Copy> {
        value: T,
        count: usize,
    }

    impl<T: Unpin + Copy> TimesPendingFuture<T> {
        fn new(value: T, count: usize) -> Self {
            Self { value, count }
        }
    }

    impl<T: Unpin + Copy> Future for TimesPendingFuture<T> {
        type Output = T;
        fn poll(
            self: core::pin::Pin<&mut Self>,
            _: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Self::Output> {
            if self.count > 0 {
                self.get_mut().count -= 1;
                core::task::Poll::Pending
            } else {
                core::task::Poll::Ready(self.value)
            }
        }
    }

    #[test]
    fn test_await_once_noblocking() {
        let future = TimesPendingFuture::new(1, 3);
        assert_eq!(await_once_noblocking!(future), None);
        let future = TimesPendingFuture::new(1, 3);
        assert_eq!(await_sync!(future), 1);
        let future = TimesPendingFuture::new(1, 0);
        assert_eq!(await_once_noblocking!(future), Some(1));
        async fn return_1() -> u32 {
            1
        }
        async fn just_await() -> u32 {
            return_1().await
        }
        assert_eq!(await_once_noblocking!(return_1()), Some(1));
        assert_eq!(await_once_noblocking!(just_await()), Some(1));
    }
}
