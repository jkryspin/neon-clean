using Microsoft.EntityFrameworkCore;

namespace NeonServer.Data;

public class NeonDb : DbContext
{
    public NeonDb(DbContextOptions<NeonDb> options) : base(options) { }

    public DbSet<Session> Sessions => Set<Session>();
    public DbSet<Script> Scripts => Set<Script>();
    public DbSet<Profile> Profiles => Set<Profile>();
    public DbSet<ActivityEvent> ActivityEvents => Set<ActivityEvent>();
    public DbSet<MarketplaceItem> MarketplaceItems => Set<MarketplaceItem>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        // Unix millisecond time storage for efficient SQL filtering
        modelBuilder.Entity<Session>()
            .Property(s => s.CreatedAt)
            .HasConversion(d => d.ToUnixTimeMilliseconds(), d => DateTimeOffset.FromUnixTimeMilliseconds(d));

        modelBuilder.Entity<ActivityEvent>()
            .Property(e => e.At)
            .HasConversion(d => d.ToUnixTimeMilliseconds(), d => DateTimeOffset.FromUnixTimeMilliseconds(d));

        // Indexes for common queries
        modelBuilder.Entity<Session>().HasIndex(s => s.Status);
        modelBuilder.Entity<ActivityEvent>().HasIndex(e => e.At);
        modelBuilder.Entity<Script>().HasIndex(s => s.Name);
    }
}

public class Session
{
    public Guid Id { get; set; }
    public string Name { get; set; } = "";
    public string? ProfileId { get; set; }
    public SessionStatus Status { get; set; }
    public DateTimeOffset CreatedAt { get; set; }
    public DateTimeOffset? LastActivity { get; set; }
    public string SettingsJson { get; set; } = "{}"; // JSON string for settings
    public double ExpRate { get; set; }
    public long MesoRate { get; set; }
}

public enum SessionStatus { Stopped, Running, Paused, Error }

public class Script
{
    public Guid Id { get; set; }
    public string Name { get; set; } = "";
    public string Code { get; set; } = "";
    public string? Description { get; set; }
    public string? Author { get; set; }
    public string? Version { get; set; }
    public bool IsPublic { get; set; }
    public DateTimeOffset CreatedAt { get; set; }
}

public class Profile
{
    public Guid Id { get; set; }
    public string Name { get; set; } = "";
    public byte[] EncryptedCredentials { get; set; } = []; // RSA-2048
    public int CharacterSlot { get; set; }
    public string World { get; set; } = "Windia";
    public int Channel { get; set; } = 1;
    public string? Proxy { get; set; } // SOCKS5/HTTP
    public byte[]? HardwareId { get; set; }
}

public class ActivityEvent
{
    public long Id { get; set; }
    public DateTimeOffset At { get; set; }
    public string Kind { get; set; } = ""; // script_execute, setting_change, session_start
    public Guid? SessionId { get; set; }
    public string? Detail { get; set; }
}

public class MarketplaceItem
{
    public Guid Id { get; set; }
    public string Name { get; set; } = "";
    public string Description { get; set; } = "";
    public string Code { get; set; } = "";
    public string Author { get; set; } = "";
    public decimal Price { get; set; }
    public bool HasFreeTrial { get; set; }
    public string? SafetyBadge { get; set; } // dangerous, network_access
    public int Downloads { get; set; }
    public DateTimeOffset CreatedAt { get; set; }
}
