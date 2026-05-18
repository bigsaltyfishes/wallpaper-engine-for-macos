macro_rules! general_smoke_cases {
    ($kind:ident, { $($case_name:ident => $case:expr),+ $(,)? }) => {
        pub mod $kind {
            $(
                #[test]
                pub fn $case_name() {
                    $case.run();
                }
            )+
        }
    };
}

pub(crate) use general_smoke_cases;
