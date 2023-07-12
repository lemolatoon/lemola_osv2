#[doc(hidden)]
pub fn dummy_waker() -> core::task::Waker {
    ::dummy_waker::dummy_waker()
}
#[macro_export]
macro_rules! await_sync {
    ($e:expr) => {{
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
}
