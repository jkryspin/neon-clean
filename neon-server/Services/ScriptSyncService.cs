using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Hosting;
using NeonServer.Data;

namespace NeonServer.Services;

public class ScriptSyncService : BackgroundService
{
    private readonly IHttpClientFactory _httpClientFactory;
    private readonly IDbContextFactory<NeonDb> _dbFactory;
    private readonly ILogger<ScriptSyncService> _logger;

    public ScriptSyncService(
        IHttpClientFactory httpClientFactory,
        IDbContextFactory<NeonDb> dbFactory,
        ILogger<ScriptSyncService> logger)
    {
        _httpClientFactory = httpClientFactory;
        _dbFactory = dbFactory;
        _logger = logger;
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        // Sync marketplace scripts every hour
        var periodicTimer = new PeriodicTimer(TimeSpan.FromHours(1));

        while (!stoppingToken.IsCancellationRequested && await periodicTimer.WaitForNextTickAsync(stoppingToken))
        {
            try
            {
                await SyncMarketplaceScriptsAsync(stoppingToken);
            }
            catch (Exception ex)
            {
                _logger.LogError(ex, "Error syncing marketplace scripts");
            }
        }
    }

    private async Task SyncMarketplaceScriptsAsync(CancellationToken ct)
    {
        _logger.LogInformation("Syncing marketplace scripts...");

        // TODO: Fetch scripts from external marketplace API
        // For now, this is a placeholder

        await using var db = await _dbFactory.CreateDbContextAsync(ct);

        // Upsert pattern: add new, update changed, keep deleted
        // Similar to ItemCatalogService in MapleWatch
    }
}
