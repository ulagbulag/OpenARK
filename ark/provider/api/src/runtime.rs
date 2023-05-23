use anyhow::{Error, Result};
use ark_api::package::{ArkPermissionKind, ArkUserSpec};
use ark_core::env;

use crate::{
    args::ActorArgs,
    builder::{
        ApplicationBuilder, ApplicationBuilderArgs, ApplicationBuilderFactory, ApplicationDevice,
        ApplicationDeviceGpu, ApplicationDeviceGpuNvidia, ApplicationDeviceIpc,
        ApplicationDevicePath, ApplicationEnvironmentVariable, ApplicationResource,
        ApplicationUserGroup, ApplicationVolume, ApplicationVolumeSource,
    },
    package::Package,
};

pub struct ApplicationRuntime<Builder> {
    builder: Builder,
    image_name_prefix: String,
}

impl<Builder> ApplicationRuntime<Builder> {
    pub fn try_default() -> Result<Self>
    where
        Builder: Default,
    {
        env::infer::<_, String>(ActorArgs::ARK_CONTAINER_IMAGE_NAME_PREFIX_KEY)
            .or_else(|_| {
                ActorArgs::ARK_CONTAINER_IMAGE_NAME_PREFIX_VALUE
                    .try_into()
                    .map_err(Error::from)
            })
            .map(Self::new)
    }

    pub fn new(image_name_prefix: String) -> Self
    where
        Builder: Default,
    {
        Self {
            builder: Default::default(),
            image_name_prefix,
        }
    }

    pub fn get_image_name(&self, namespace: &str, name: &str, version: &str) -> String {
        let name_prefix = &self.image_name_prefix;
        format!("{name_prefix}/{namespace}/ark-package-{name}:{version}")
    }

    pub fn get_image_name_from_package(&self, namespace: &str, package: &Package) -> String {
        let version = package.resource.get_image_version();
        self.get_image_name(namespace, &package.name, version)
    }
}

impl<'args, Builder> ApplicationRuntime<Builder>
where
    Builder: ApplicationBuilderFactory<'args>,
{
    pub async fn spawn(
        &self,
        args: <Builder as ApplicationBuilderFactory<'args>>::Args,
        ApplicationRuntimeCtx {
            namespace,
            node_name,
            package,
            command_line_arguments,
            sync,
        }: ApplicationRuntimeCtx<'args>,
    ) -> Result<()> {
        let Package { name, resource } = package;
        let ArkUserSpec {
            name: username,
            uid,
            ..
        } = &resource.spec.user;

        let dst_home = format!("/home/{username}");

        let mut builder = self
            .builder
            .create_builder(
                args,
                ApplicationBuilderArgs {
                    command_line_arguments,
                    image_name: self.get_image_name_from_package(namespace, package),
                    user: &resource.spec.user,
                },
            )
            .await?;

        if let Some(node_name) = node_name {
            builder.add(ApplicationResource::NodeName(node_name))?;
        }

        // specify home directory
        builder.add(ApplicationResource::EnvironmentVariable(
            ApplicationEnvironmentVariable {
                key: "HOME",
                value: &dst_home,
            },
        ))?;

        for permission in &resource.spec.permissions {
            match &permission.name {
                ArkPermissionKind::Audio => {
                    builder.add(ApplicationResource::UserGroup(ApplicationUserGroup::Name(
                        "audio",
                    )))?;
                    builder.add(ApplicationResource::Device(ApplicationDevice::Path(
                        ApplicationDevicePath { src: "/dev/snd" },
                    )))?;
                    builder.add(ApplicationResource::Volume(ApplicationVolume {
                        src: ApplicationVolumeSource::HostPath(None),
                        dst_path: "/etc/pulse",
                        read_only: true,
                    }))?;
                }
                ArkPermissionKind::Graphics => {
                    builder.add(ApplicationResource::Device(ApplicationDevice::Gpu(
                        ApplicationDeviceGpu::Nvidia(ApplicationDeviceGpuNvidia::All),
                    )))?;
                    builder.add(ApplicationResource::Device(ApplicationDevice::Ipc(
                        ApplicationDeviceIpc::Host,
                    )))?;
                    builder.add(ApplicationResource::UserGroup(ApplicationUserGroup::Name(
                        "render",
                    )))?;
                    builder.add(ApplicationResource::UserGroup(ApplicationUserGroup::Name(
                        "video",
                    )))?;
                    builder.add(ApplicationResource::EnvironmentVariable(
                        ApplicationEnvironmentVariable {
                            key: "DISPLAY",
                            value: ":0",
                        },
                    ))?;
                    builder.add(ApplicationResource::EnvironmentVariable(
                        ApplicationEnvironmentVariable {
                            key: "NVIDIA_DRIVER_CAPABILITIES",
                            value: "all",
                        },
                    ))?;
                    builder.add(ApplicationResource::EnvironmentVariable(
                        ApplicationEnvironmentVariable {
                            key: "NVIDIA_VISIBLE_DEVICES",
                            value: "all",
                        },
                    ))?;
                    builder.add(ApplicationResource::EnvironmentVariable(
                        ApplicationEnvironmentVariable {
                            key: "XDG_RUNTIME_DIR",
                            value: &format!("/run/user/{uid}"),
                        },
                    ))?;
                    builder.add(ApplicationResource::Device(ApplicationDevice::Path(
                        ApplicationDevicePath { src: "/dev/dri" },
                    )))?;
                    builder.add(ApplicationResource::Volume(ApplicationVolume {
                        src: ApplicationVolumeSource::UserHome(Some(&format!(
                            "./.local/share/ark/{name}/"
                        ))),
                        dst_path: &dst_home,
                        read_only: false,
                    }))?;
                    builder.add(ApplicationResource::Volume(ApplicationVolume {
                        src: ApplicationVolumeSource::HostPath(None),
                        dst_path: "/usr/share/egl/egl_external_platform.d",
                        read_only: true,
                    }))?;
                    builder.add(ApplicationResource::Volume(ApplicationVolume {
                        src: ApplicationVolumeSource::HostPath(None),
                        dst_path: "/usr/share/glvnd/egl_vendor.d",
                        read_only: true,
                    }))?;
                    builder.add(ApplicationResource::Volume(ApplicationVolume {
                        src: ApplicationVolumeSource::HostPath(None),
                        dst_path: "/usr/share/vulkan/icd.d",
                        read_only: true,
                    }))?;
                    builder.add(ApplicationResource::Volume(ApplicationVolume {
                        src: ApplicationVolumeSource::HostPath(None),
                        dst_path: "/run/dbus",
                        read_only: true,
                    }))?;
                    builder.add(ApplicationResource::Volume(ApplicationVolume {
                        src: ApplicationVolumeSource::HostPath(None),
                        dst_path: &format!("/run/user/{uid}"),
                        read_only: false,
                    }))?;
                    builder.add(ApplicationResource::Volume(ApplicationVolume {
                        src: ApplicationVolumeSource::HostPath(None),
                        dst_path: "/tmp/.ICE-unix",
                        read_only: true,
                    }))?;
                    builder.add(ApplicationResource::Volume(ApplicationVolume {
                        src: ApplicationVolumeSource::HostPath(None),
                        dst_path: "/tmp/.X11-unix",
                        read_only: true,
                    }))?;
                }
            }
        }
        builder.spawn(sync).await
    }
}

pub struct ApplicationRuntimeCtx<'a> {
    pub namespace: &'a str,
    pub node_name: Option<&'a str>,
    pub package: &'a Package,
    pub command_line_arguments: &'a [String],
    pub sync: bool,
}