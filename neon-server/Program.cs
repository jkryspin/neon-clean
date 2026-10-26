using Microsoft.AspNetCore.Builder.Extensions;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Http.HttpResults;
using Microsoft.EntityFrameworkCore;
using NeonServer.Data;
using NeonServer.Hubs;
using NeonServer.Services;

var builder = WebApplication.CreateBuilder(args);

// Add services
builder.Services.AddControllers();
builder.Services.AddEndpointsApiExplorer();
builder.Services.AddSwaggerGen();
builder.Services.AddHttpClient();

// Database
builder.Services.AddDbContextFactory<NeonDb>(options =>
    options.UseSqlite("Data Source=neon.db"));

// SignalR
builder.Services.AddSignalR();

// Background services
builder.Services.AddHostedService<ScriptSyncService>();
builder.Services.AddHostedService<ActivityLogger>();

var app = builder.Build();

// Create database and apply migrations
using (var scope = app.Services.CreateScope())
{
    var dbFactory = scope.ServiceProvider.GetRequiredService<IDbContextFactory<NeonDb>>();
    using (var db = dbFactory.CreateDbContext())
    {
        db.Database.EnsureCreated();
    }
}

// Middleware
app.UseSwagger();
app.UseSwaggerUI();
app.UseCors(policy => policy.AllowAnyOrigin().AllowAnyMethod().AllowAnyHeader());
app.UseStaticFiles();

// Map endpoints
app.MapControllers();
app.MapHub<NeonHub>("/neon");

app.Run();
