# Runs polaris-tui full screen on a Linux virtual console, without any
# display server. Session handling mirrors the nixpkgs `cage` kiosk module.
self:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.polaris-dashboard;
  settingsFormat = pkgs.formats.toml { };
  configFile =
    if cfg.configFile != null then
      cfg.configFile
    else
      settingsFormat.generate "polaris.toml" cfg.settings;
in
{
  options.services.polaris-dashboard = {
    enable = lib.mkEnableOption "the Polaris console dashboard";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.polaris-tui;
      defaultText = lib.literalExpression "polaris.packages.\${system}.polaris-tui";
      description = "Polaris TUI package.";
    };

    configFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = lib.literalExpression "./polaris.toml";
      description = "Dashboard configuration file. Takes precedence over `settings`.";
    };

    settings = lib.mkOption {
      type = settingsFormat.type;
      default = { };
      description = "Dashboard configuration as a Nix attribute set, rendered to TOML.";
    };

    grafanaTokenFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = "/run/secrets/polaris-grafana-token";
      description = ''
        File holding a Grafana service account token (Viewer role is enough).
        Passed to the service as the `grafana-token` systemd credential, so it
        can stay readable by root only.
      '';
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "polaris-dashboard";
      description = "User owning the console session.";
    };

    tty = lib.mkOption {
      type = lib.types.str;
      default = "tty1";
      description = "Virtual console the dashboard is drawn on.";
    };

    consoleFont = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = "ter-v16n";
      description = ''
        Terminus console font; its size sets the grid resolution
        (ter-v16n on 1280x400 gives 160x25 cells, ter-v24n gives 106x16).
        `null` leaves the console font untouched.
      '';
    };

    videoMode = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "HDMI-A-1:1280x400@60";
      description = "Forced KMS video mode, for screens with a missing or wrong EDID.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.configFile != null || cfg.settings != { };
        message = "services.polaris-dashboard needs either configFile or settings.";
      }
    ];

    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.user;
    };
    users.groups.${cfg.user} = { };

    # The kernel blanks idle consoles after 10 minutes by default.
    boot.kernelParams = [
      "consoleblank=0"
    ]
    ++ lib.optional (cfg.videoMode != null) "video=${cfg.videoMode}";
    # Keep kernel messages from drawing over the dashboard.
    boot.consoleLogLevel = lib.mkDefault 3;

    console = lib.mkIf (cfg.consoleFont != null) {
      font = cfg.consoleFont;
      packages = [ pkgs.terminus_font ];
      earlySetup = true;
    };

    systemd.services."getty@${cfg.tty}".enable = false;

    security.pam.services.polaris-dashboard.text = ''
      auth    required pam_unix.so nullok
      account required pam_unix.so
      session required pam_unix.so
      session required pam_env.so conffile=/etc/pam/environment readenv=0
      session required ${config.systemd.package}/lib/security/pam_systemd.so
    '';

    systemd.services.polaris-dashboard = {
      description = "Polaris console dashboard";
      after = [
        "systemd-user-sessions.service"
        "getty@${cfg.tty}.service"
        "network-online.target"
      ];
      wants = [ "network-online.target" ];
      conflicts = [ "getty@${cfg.tty}.service" ];
      wantedBy = [ "multi-user.target" ];
      environment.TERM = "linux";
      restartTriggers = [ configFile ];
      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} ${configFile}";
        User = cfg.user;
        PAMName = "polaris-dashboard";
        TTYPath = "/dev/${cfg.tty}";
        TTYReset = true;
        TTYVHangup = true;
        TTYVTDisallocate = true;
        StandardInput = "tty-fail";
        StandardOutput = "tty";
        StandardError = "journal";
        UtmpIdentifier = cfg.tty;
        UtmpMode = "user";
        Restart = "always";
        RestartSec = 5;
        LoadCredential = lib.optional (
          cfg.grafanaTokenFile != null
        ) "grafana-token:${cfg.grafanaTokenFile}";
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
      };
    };
  };
}
