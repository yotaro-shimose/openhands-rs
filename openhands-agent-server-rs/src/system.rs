use serde::Serialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::{Disks, Pid, System};

#[derive(Serialize)]
pub struct MemoryStats {
    pub rss: u64,
    pub vms: u64,
    pub percent: f32,
}

#[derive(Serialize)]
pub struct DiskStats {
    pub total: u64,
    pub used: u64,
    pub free: u64,
    pub percent: f32,
}

#[derive(Serialize)]
pub struct IoStats {
    pub read_bytes: u64,
    pub write_bytes: u64,
}

#[derive(Serialize)]
pub struct Resources {
    pub cpu_percent: f32,
    pub memory: MemoryStats,
    pub disk: DiskStats,
    pub io: IoStats,
}

#[derive(Serialize)]
pub struct SystemInfo {
    pub uptime: f64,
    pub idle_time: f64,
    pub resources: Resources,
}

lazy_static::lazy_static! {
    static ref START_TIME: f64 = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
}

pub async fn get_system_info() -> SystemInfo {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    
    let uptime = current_time - *START_TIME;
    let idle_time = uptime; 

    let mut sys = System::new_all();
    
    // Disks are separate in 0.30
    let disks = Disks::new_with_refreshed_list();

    let pid = Pid::from_u32(std::process::id());
    
    // Refresh for CPU
    // refresh_pids takes a slice of Pids
    sys.refresh_pids(&[pid]);
    tokio::time::sleep(Duration::from_millis(100)).await;
    sys.refresh_pids(&[pid]);

    let (cpu_percent, memory_stats, io_stats) = if let Some(process) = sys.process(pid) {
        let cpu = process.cpu_usage(); 
        
        let mem = MemoryStats {
            rss: process.memory(),
            vms: process.virtual_memory(),
            percent: 0.0, 
        };
        
        // Process disk usage might be available depending on OS support in sysinfo
        let disk_usage = process.disk_usage();
        let io = IoStats {
            read_bytes: disk_usage.read_bytes,
            write_bytes: disk_usage.written_bytes,
        };
        (cpu, mem, io)
    } else {
        (0.0, MemoryStats { rss:0, vms:0, percent:0.0 }, IoStats { read_bytes:0, write_bytes:0 })
    };

    let mut disk_stats = DiskStats { total: 0, used: 0, free: 0, percent: 0.0 };
    
    // Find root disk
    for disk in &disks {
        if disk.mount_point() == std::path::Path::new("/") {
            disk_stats.total = disk.total_space();
            disk_stats.free = disk.available_space();
            disk_stats.used = disk_stats.total - disk_stats.free;
            // Avoid division by zero
            if disk_stats.total > 0 {
                disk_stats.percent = (disk_stats.used as f64 / disk_stats.total as f64 * 100.0) as f32;
            }
            break;
        }
    }

    SystemInfo {
        uptime,
        idle_time,
        resources: Resources {
            cpu_percent,
            memory: memory_stats,
            disk: disk_stats,
            io: io_stats,
        },
    }
}

