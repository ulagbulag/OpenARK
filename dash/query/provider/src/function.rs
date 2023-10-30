use std::{fmt, sync::Arc};

use anyhow::Result;
use dash_api::pipe::PipeSpec;
use dash_pipe_provider::{
    deltalake::{
        arrow::datatypes::{DataType, Schema},
        datafusion::{
            error::DataFusionError,
            logical_expr::{ScalarUDF, Signature, TypeSignature, Volatility},
        },
    },
    messengers::Messenger,
    GenericStatelessRemoteFunction, Name,
};
use futures::executor::block_on;
use inflector::Inflector;
use tokio::{spawn, sync::oneshot};
use tracing::debug;

#[derive(Debug)]
pub(crate) struct DashFunctionTemplate {
    name: Name,
    model_in: Name,
    spec: RemotePipeSpec,
}

impl DashFunctionTemplate {
    pub(crate) fn new(name: Name, model_in: Name, spec: RemotePipeSpec) -> Result<Self> {
        Ok(Self {
            name,
            model_in,
            spec,
        })
    }

    pub(crate) async fn try_into_udf(self, messenger: &dyn Messenger) -> Result<ScalarUDF> {
        let info = self.to_string();

        let Self {
            name,
            model_in,
            spec:
                PipeSpec {
                    input: input_schema,
                    output: output_schema,
                    exec: (),
                },
        } = self;

        let input = DataType::Struct(input_schema.fields().clone());
        let inputs = input_schema
            .fields()
            .iter()
            .map(|field| field.data_type().clone())
            .collect();
        let output = DataType::Struct(output_schema.fields().clone());

        let function = GenericStatelessRemoteFunction::try_new(messenger, model_in)
            .await?
            .into_delta(input_schema, output_schema);

        Ok(ScalarUDF {
            name: name.to_snake_case(),
            signature: Signature {
                type_signature: TypeSignature::OneOf(vec![
                    TypeSignature::Exact(vec![input]),
                    TypeSignature::Exact(inputs),
                ]),
                volatility: Volatility::Immutable,
            },
            return_type: {
                let return_type = Arc::new(output);
                Arc::new(move |_| Ok(Arc::clone(&return_type)))
            },
            fun: Arc::new(move |inputs| {
                debug!("Calling function: {info}");

                let (tx, rx) = oneshot::channel();

                let function = function.clone();
                let inputs = inputs.to_vec();
                spawn(async move {
                    let result = function.call(&inputs).await;
                    let _ = tx.send(result);
                });

                block_on(rx)
                    .map_err(|error| DataFusionError::External(error.into()))
                    .and_then(|result| {
                        result.map_err(|error| DataFusionError::External(error.into()))
                    })
            }),
        })
    }
}

impl fmt::Display for DashFunctionTemplate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            name,
            model_in: _,
            spec:
                PipeSpec {
                    input,
                    output: _,
                    exec: _,
                },
        } = self;

        write!(f, "{name}(")?;
        for (index, name) in input.fields().iter().map(|field| field.name()).enumerate() {
            if index > 0 {
                write!(f, ", ")?;
            }
            name.fmt(f)?;
        }
        write!(f, ")")
    }
}

type RemotePipeSpec = PipeSpec<Arc<Schema>, ()>;