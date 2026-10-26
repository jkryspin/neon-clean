using Microsoft.AspNetCore.Mvc;
using Microsoft.EntityFrameworkCore;
using NeonServer.Data;
using System.Text.Json;

namespace NeonServer.Controllers;

[ApiController]
[Route("api/sessions")]
public class SessionsController : ControllerBase
{
    private readonly IDbContextFactory<NeonDb> _dbFactory;
    private readonly ILogger<SessionsController> _logger;

    public SessionsController(
        IDbContextFactory<NeonDb> dbFactory,
        ILogger<SessionsController> logger)
    {
        _dbFactory = dbFactory;
        _logger = logger;
    }

    [HttpGet]
    public async Task<IActionResult> GetAll(CancellationToken ct)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(ct);
        var sessions = await db.Sessions.ToListAsync(ct);
        return Ok(sessions);
    }

    [HttpGet("{id}")]
    public async Task<IActionResult> Get(Guid id, CancellationToken ct)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(ct);
        var session = await db.Sessions.FindAsync([id], ct);
        if (session == null)
        {
            return NotFound();
        }
        return Ok(session);
    }

    [HttpPost]
    public async Task<IActionResult> Create([FromBody] CreateSessionRequest request, CancellationToken ct)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(ct);

        var session = new Session
        {
            Id = Guid.NewGuid(),
            Name = request.Name,
            ProfileId = request.ProfileId,
            Status = SessionStatus.Stopped,
            CreatedAt = DateTimeOffset.UtcNow,
            SettingsJson = request.Settings != null ? JsonSerializer.Serialize(request.Settings) : "{}"
        };

        db.Sessions.Add(session);
        await db.SaveChangesAsync(ct);

        return CreatedAtAction(nameof(Get), new { id = session.Id }, session);
    }

    [HttpPatch("{id}")]
    public async Task<IActionResult> Update(Guid id, [FromBody] UpdateSessionRequest request, CancellationToken ct)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(ct);
        var session = await db.Sessions.FindAsync([id], ct);
        if (session == null)
        {
            return NotFound();
        }

        if (request.Name != null)
            session.Name = request.Name;
        if (request.ProfileId != null)
            session.ProfileId = request.ProfileId;
        if (request.Settings != null)
            session.SettingsJson = JsonSerializer.Serialize(request.Settings);

        session.LastActivity = DateTimeOffset.UtcNow;
        await db.SaveChangesAsync(ct);

        return Ok(session);
    }

    [HttpPost("{id}/toggle")]
    public async Task<IActionResult> Toggle(Guid id, CancellationToken ct)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(ct);
        var session = await db.Sessions.FindAsync([id], ct);
        if (session == null)
        {
            return NotFound();
        }

        session.Status = session.Status == SessionStatus.Running
            ? SessionStatus.Stopped
            : SessionStatus.Running;
        session.LastActivity = DateTimeOffset.UtcNow;
        await db.SaveChangesAsync(ct);

        return Ok(new { toggled = true, status = session.Status });
    }

    [HttpDelete("{id}")]
    public async Task<IActionResult> Delete(Guid id, CancellationToken ct)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(ct);
        var session = await db.Sessions.FindAsync([id], ct);
        if (session == null)
        {
            return NotFound();
        }

        db.Sessions.Remove(session);
        await db.SaveChangesAsync(ct);

        return NoContent();
    }
}

public record CreateSessionRequest(
    string Name,
    string? ProfileId,
    Dictionary<string, object>? Settings
);

public record UpdateSessionRequest(
    string? Name,
    string? ProfileId,
    Dictionary<string, object>? Settings
);
