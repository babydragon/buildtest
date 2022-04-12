use std::io::Write;
use anyhow::Result;
use async_channel::Sender;
use bollard::container::{AttachContainerOptions, AttachContainerResults, Config, LogOutput, LogsOptions, RemoveContainerOptions, WaitContainerOptions};
use bollard::models::BuildInfo;
use futures::TryStreamExt;
use futures::stream::StreamExt;

pub struct Docker {
    client: bollard::Docker,
}

impl Docker {
    pub fn new() -> Docker {
        let connect = bollard::Docker::connect_with_local_defaults().expect("fail to init docker");
        Docker {
            client: connect
        }
    }

    pub async fn build(&self, name: String, s: Option<Sender<String>>) -> Result<()> {
        let mut image_build_stream = self.client.build_image(bollard::image::BuildImageOptions {
            dockerfile: "Dockerfile".to_string(),
            t:name,
            pull: true,
            rm: true,
            ..Default::default()
        }, None, Some(self.generate_dockerfile().into()));

        while let Some(Ok(msg)) =  image_build_stream.next().await {
            dbg!("Message: {:?}", &msg);
            if let Some(ref sender) = s {
                if let Some(stream) = msg.stream {
                    sender.send(stream).await?;
                }

                if let Some(status) = msg.status {
                    sender.send(status).await?;
                }
            }
        }

        Ok(())
    }

    pub async fn run(&self, image: String, cmd: Vec<String>, s: Option<Sender<String>>) -> Result<String> {
        // 1. pull image
        dbg!("start to pull image: {}", &image);
       self.client.create_image(Some(bollard::image::CreateImageOptions {
           from_image: image.clone(),
            ..Default::default()
        }), None, None).try_collect::<Vec<_>>().await?;
        dbg!("image[{}] pull success", &image);

        let config = Config {
            image: Some(image),
            cmd: Some(cmd),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };

        // 2. create container
        dbg!("start to create container");
        let container_id = self.client.create_container::<String, String>(None, config).await?.id;
        dbg!("container[{}] create success", &container_id);

        // 3. start container
        dbg!("start to start container {}", &container_id);
        self.client.start_container::<String>(&container_id, None).await?;
        dbg!("container[{}] start success", &container_id);

        // 4. block wait container stop
        dbg!("start to wait container {}", &container_id);
        let mut wait_resp_stream = self.client.wait_container(&container_id,
                                                              Some(WaitContainerOptions { condition: "not-running" }));
        if let Some(Ok(wait_resp)) = wait_resp_stream.next().await  {
            dbg!("container[{}] exit with code: {}", &container_id, wait_resp.status_code);
            if s.is_some() && wait_resp.status_code != 0 {
                dbg!("container[{}] exit nonzero, start to fetch log", &container_id);
                let sender = s.unwrap();
                let mut log_stream = self.client.logs(&container_id, Some(LogsOptions::<String> {
                    stdout: true,
                    stderr: true,
                    timestamps: true,
                    ..Default::default()
                }));
                while let Some(Ok(msg)) =  log_stream.next().await {
                    sender.send(msg.to_string()).await?;
                }
            }
        }

        // 5. remove container
        dbg!("start to remove container {}", &container_id);
        self.client.remove_container(&container_id, Some(RemoveContainerOptions{
            force: true,
            ..Default::default()
        })).await?;
        dbg!("container[{}] remove success", &container_id);

        Ok("".to_string())
    }

    fn generate_dockerfile(&self) -> Vec<u8> {
        let content = "FROM alpine:3.15\nRUN touch build-test.txt";

        let mut header = tar::Header::new_gnu();
        header.set_path("Dockerfile");
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();

        let mut tar = tar::Builder::new(Vec::new());
        tar.append(&header, content.as_bytes()).expect("fail to append");

        let uncompressed = tar.into_inner().unwrap();

        let mut c = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        c.write_all(&uncompressed).expect("fail to write");

        c.finish().expect("fail to compress")
    }

}
