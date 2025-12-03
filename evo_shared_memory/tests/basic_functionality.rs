//! Basic functionality tests for EVO Shared Memory

use evo_shared_memory::{SHM_MIN_SIZE, SegmentReader, SegmentWriter, ShmError, ShmResult};

#[test]
fn test_basic_write_read() -> ShmResult<()> {
    let segment_name = "test_basic_write_read";
    let test_data = b"Hello, EVO!";

    // Create writer and write data
    let mut writer = SegmentWriter::create(segment_name, SHM_MIN_SIZE)?;
    writer.write(test_data)?;

    // Create reader and read data
    let mut reader = SegmentReader::attach(segment_name)?;
    let read_data = reader.read()?;

    // Compare only the written data length
    assert_eq!(&read_data[..test_data.len()], test_data);
    Ok(())
}

#[test]
fn test_multiple_writes() -> ShmResult<()> {
    let segment_name = "test_multiple_writes";
    let mut writer = SegmentWriter::create(segment_name, SHM_MIN_SIZE)?;
    let mut reader = SegmentReader::attach(segment_name)?;

    for i in 0..10 {
        let test_data = format!("Message {}", i);
        writer.write(test_data.as_bytes())?;

        let read_data = reader.read()?;
        assert_eq!(&read_data[..test_data.len()], test_data.as_bytes());
    }

    Ok(())
}

#[test]
fn test_concurrent_readers() -> ShmResult<()> {
    let segment_name = "test_concurrent_readers";
    let test_data = b"Concurrent test data";

    // Create writer
    let mut writer = SegmentWriter::create(segment_name, SHM_MIN_SIZE)?;
    writer.write(test_data)?;

    let handles: Vec<_> = (0..3)
        .map(|i| {
            let segment_name = segment_name.to_string();
            let test_data = test_data.to_vec();
            std::thread::spawn(move || -> ShmResult<()> {
                let mut reader = SegmentReader::attach(&segment_name)?;
                let read_data = reader.read()?;
                assert_eq!(&read_data[..test_data.len()], &test_data[..]);
                println!("Reader {} OK", i);
                Ok(())
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap()?;
    }

    Ok(())
}

#[test]
fn test_invalid_segment_size() {
    // Test that invalid sizes are properly rejected
    let result = SegmentWriter::create("test_invalid_size", 0);
    assert!(result.is_err());

    if let Err(e) = result {
        match e {
            ShmError::InvalidSize { .. } => {
                // Expected error
            }
            other => panic!("Expected InvalidSize error, got: {:?}", other),
        }
    }
}

#[test]
fn test_segment_not_found() {
    // Test that attaching to non-existent segment fails
    let result = SegmentReader::attach("non_existent_segment");
    assert!(result.is_err());

    if let Err(e) = result {
        match e {
            ShmError::NotFound { .. } => {
                // Expected error
            }
            other => panic!("Expected NotFound error, got: {:?}", other),
        }
    }
}

#[test]
fn test_large_data() -> ShmResult<()> {
    let segment_name = "test_large_data";
    let test_data = vec![0xAB; 2048]; // 2KB of data

    let mut writer = SegmentWriter::create(segment_name, 8192)?; // 8KB segment
    writer.write(&test_data)?;

    let mut reader = SegmentReader::attach(segment_name)?;
    let read_data = reader.read()?;

    assert_eq!(&read_data[..test_data.len()], &test_data[..]);
    Ok(())
}

#[test]
fn test_empty_data() -> ShmResult<()> {
    let segment_name = "test_empty_data";
    let test_data = b"";

    let mut writer = SegmentWriter::create(segment_name, SHM_MIN_SIZE)?;
    writer.write(test_data)?;

    let mut reader = SegmentReader::attach(segment_name)?;
    let read_data = reader.read()?;

    // For empty data, just check that we can read without error
    assert_eq!(&read_data[..test_data.len()], test_data);
    Ok(())
}

#[test]
fn test_data_consistency() -> ShmResult<()> {
    let segment_name = "test_data_consistency";
    let test_data = vec![0xAA; 1024]; // 1KB of repeated pattern

    let mut writer = SegmentWriter::create(segment_name, SHM_MIN_SIZE)?;
    writer.write(&test_data)?;

    let mut reader = SegmentReader::attach(segment_name)?;
    let read_data = reader.read()?;

    assert_eq!(&read_data[..test_data.len()], &test_data[..]);

    // Verify all bytes in the written portion have expected value
    for &byte in &read_data[..test_data.len()] {
        assert_eq!(byte, 0xAA);
    }

    Ok(())
}

#[test]
fn test_writer_reader_lifecycle() -> ShmResult<()> {
    let segment_name = "test_lifecycle";

    // Test 1: Writer and reader in same scope
    {
        let mut writer = SegmentWriter::create(segment_name, SHM_MIN_SIZE)?;
        writer.write(b"Initial data")?;

        let mut reader = SegmentReader::attach(segment_name)?;
        let data = reader.read()?;
        assert_eq!(&data[..12], b"Initial data");

        // Update data while reader is still active
        writer.write(b"Updated data")?;
        let data = reader.read()?;
        assert_eq!(&data[..12], b"Updated data");
    } // Both go out of scope

    // Test 2: Create new segment with different name for new lifecycle
    let segment_name2 = "test_lifecycle_2";
    {
        let mut writer = SegmentWriter::create(segment_name2, SHM_MIN_SIZE)?;
        writer.write(b"New data")?;

        let mut reader = SegmentReader::attach(segment_name2)?;
        let data = reader.read()?;
        assert_eq!(&data[..8], b"New data");
    }

    Ok(())
}
