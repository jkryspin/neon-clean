using System.Threading.Channels;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Hosting;
using NeonServer.Data;

namespace NeonServer.Services;

public class ActivityLogger : BackgroundService
{
    private readonly IDbContextFactory<NeonDb> _dbFactory;
    private readonly ILogger<ActivityLogger> _logger;
    private readonly Channel<ActivityEvent> _events;

    public ActivityLogger(
        IDbContextFactory<NeonDb> dbFactory,
        ILogger<ActivityLogger> logger)
    {
        _dbFactory = dbFactory;
        _logger = logger;
        // Channel-based event buffering (20,000 capacity, drop-on-full)
        _events = Channel.CreateBounded<ActivityEvent>(
            new BoundedChannelOptions(20_000)
            {
                FullMode = BoundedChannelFullMode.DropWrite
            });
    }

    public void Log(ActivityEvent @event)
    {
        _events.Writer.TryWrite(@event);
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        // Batch writes every 5 seconds
        var periodicTimer = new PeriodicTimer(TimeSpan.FromSeconds(5));

        while (!stoppingToken.IsCancellationRequested && await periodicTimer.WaitForNextTickAsync(stoppingToken))
        {
            try
            {
                await FlushEventsAsync(stoppingToken);
            }
            catch (Exception ex)
            {
                _logger.LogError(ex, "Error flushing activity events");
            }
        }
    }

    private async Task FlushEventsAsync(CancellationToken ct)
    {
        var events = new List<ActivityEvent>();

        // Collect up to 5,000 events per batch
        while (events.Count < 5000 && _events.Reader.TryRead(out var @event))
        {
            events.Add(@event);
        }

        if (events.Count == 0)
        {
            return;
        }

        await using var db = await _dbFactory.CreateDbContextAsync(ct);
        db.ActivityEvents.AddRange(events);
        await db.SaveChangesAsync(ct);

        _logger.LogDebug("Flushed {Count} activity events", events.Count);
    }
}
