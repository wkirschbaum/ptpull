//! Integration tests using the mock PTP-IP camera server

mod mock_camera;

use std::net::Ipv4Addr;

use ptpull::camera::operations::Camera;
use ptpull::camera::types::CameraInfo;

fn camera_info(addr: std::net::SocketAddr) -> CameraInfo {
    CameraInfo {
        ip: Ipv4Addr::LOCALHOST,
        port: addr.port(),
        device_info: None,
    }
}

#[tokio::test]
async fn test_connect_and_get_device_info() {
    let addr = mock_camera::start_mock_camera().await;
    let camera = Camera::connect(camera_info(addr)).await.unwrap();

    let info = camera.device_info.as_ref().unwrap();
    assert_eq!(info.manufacturer, "MockCorp");
    assert_eq!(info.model, "MockCam X100");
    assert_eq!(info.serial_number, "MOCK-001");
    assert!(info.supports_partial_object());

    camera.disconnect().await.unwrap();
}

#[tokio::test]
async fn test_list_storages() {
    let addr = mock_camera::start_mock_camera().await;
    let mut camera = Camera::connect(camera_info(addr)).await.unwrap();

    let storages = camera.list_storages().await.unwrap();
    assert_eq!(storages.len(), 1);
    assert_eq!(storages[0].volume_label, "MOCK_SD");
    assert!(storages[0].is_removable());

    camera.disconnect().await.unwrap();
}

#[tokio::test]
async fn test_list_objects() {
    let addr = mock_camera::start_mock_camera().await;
    let mut camera = Camera::connect(camera_info(addr)).await.unwrap();

    let storages = camera.list_storages().await.unwrap();
    let objects = camera.list_objects(storages[0].storage_id).await.unwrap();

    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].filename, "DSC_0001.JPG");
    assert_eq!(objects[0].compressed_size, 4096);
    assert!(objects[0].is_image());
    assert!(!objects[0].is_folder());

    camera.disconnect().await.unwrap();
}

#[tokio::test]
async fn test_download_object() {
    let addr = mock_camera::start_mock_camera().await;
    let mut camera = Camera::connect(camera_info(addr)).await.unwrap();

    let storages = camera.list_storages().await.unwrap();
    let objects = camera.list_objects(storages[0].storage_id).await.unwrap();

    let tmp_dir = tempfile::tempdir().unwrap();
    let obj = &objects[0];

    // Track progress
    let progress_bytes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let pb = progress_bytes.clone();
    let progress_fn: ptpull::camera::operations::ProgressFn =
        Box::new(move |downloaded, _total| {
            pb.store(downloaded, std::sync::atomic::Ordering::Relaxed);
        });

    let dest = camera
        .download_object(obj, tmp_dir.path(), Some(progress_fn))
        .await
        .unwrap();

    // Verify file was downloaded
    assert!(dest.exists());
    assert_eq!(dest.file_name().unwrap(), "DSC_0001.JPG");

    let contents = std::fs::read(&dest).unwrap();
    assert_eq!(contents.len(), 4096);

    // Verify content matches the deterministic pattern from mock
    for (i, byte) in contents.iter().enumerate() {
        assert_eq!(*byte, (i % 256) as u8, "byte mismatch at offset {i}");
    }

    // Verify progress was reported
    let final_progress = progress_bytes.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(final_progress, 4096);

    camera.disconnect().await.unwrap();
}
