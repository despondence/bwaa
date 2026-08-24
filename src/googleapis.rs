pub mod google {
    pub mod r#type {
        tonic::include_proto!("google.r#type");
    }

    pub mod rpc {
        tonic::include_proto!("google.rpc");
    }

    pub mod api {
        tonic::include_proto!("google.api");
    }

    pub mod ai {
        pub mod generativelanguage {
            pub mod v1beta {
                tonic::include_proto!("google.ai.generativelanguage.v1beta");
            }

            pub mod v1alpha {
                tonic::include_proto!("google.ai.generativelanguage.v1alpha");
            }
        }
    }
}
