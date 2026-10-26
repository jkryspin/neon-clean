using Microsoft.AspNetCore.SignalR;

namespace NeonServer.Hubs;

public class NeonHub : Hub
{
    private readonly ILogger<NeonHub> _logger;

    public NeonHub(ILogger<NeonHub> logger)
    {
        _logger = logger;
    }

    public override async Task OnConnectedAsync()
    {
        var clientId = Context.ConnectionId;
        _logger.LogInformation("Client connected: {ClientId}", clientId);
        await Clients.All.SendAsync("ClientJoined", clientId);
        await base.OnConnectedAsync();
    }

    public override async Task OnDisconnectedAsync(Exception? exception)
    {
        var clientId = Context.ConnectionId;
        _logger.LogInformation("Client disconnected: {ClientId}", clientId);
        if (exception != null)
        {
            _logger.LogError(exception, "Client disconnected with error");
        }
        await Clients.All.SendAsync("ClientLeft", clientId);
        await base.OnDisconnectedAsync(exception);
    }

    // Session updates
    public async Task UpdateSession(Guid sessionId, object settings)
    {
        await Clients.All.SendAsync("SessionUpdated", sessionId, settings);
    }

    public async Task UpdateStats(Guid sessionId, double expRate, long mesoRate)
    {
        await Clients.Caller.SendAsync("StatsUpdated", new
        {
            sessionId,
            expRate,
            mesoRate,
            timestamp = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds()
        });
    }

    // Script execution
    public async Task ScriptStarted(Guid sessionId, string scriptName)
    {
        await Clients.All.SendAsync("ScriptStarted", new { sessionId, scriptName });
    }

    public async Task ScriptStopped(Guid sessionId, string scriptName)
    {
        await Clients.All.SendAsync("ScriptStopped", new { sessionId, scriptName });
    }

    // Activity log
    public async Task LogActivity(string kind, Guid? sessionId, string? detail)
    {
        await Clients.All.SendAsync("ActivityLogged", new
        {
            kind,
            sessionId,
            detail,
            timestamp = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds()
        });
    }

    // Settings sync
    public async Task SyncSettings(Guid sessionId, Dictionary<string, object> settings)
    {
        await Clients.Caller.SendAsync("SettingsSynced", settings);
    }
}
