//! The Vulkan renderer's development viewer; see `torch_client::vk::viewer`.

fn main() {
    std::process::exit(torch_client::vk::viewer::run(
        std::env::args().skip(1).collect(),
    ));
}
