# Runs polaris-tui full screen on a virtual console, without any display server.
#
# Two consoles are supported:
# - kmscon (default): DRM terminal rendering TrueType fonts, so braille and
#   block glyphs used by charts display properly. kmscon needs root for DRM
#   and input devices; the dashboard itself runs unprivileged under setpriv.
# - linux: the kernel framebuffer console. No extra daemon, but its fonts hold
#   512 glyphs at most, which leaves out braille and most block characters.
#   Session handling mirrors the nixpkgs `cage` kiosk module.
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
  isKmscon = cfg.console == "kmscon";
  dashboardCommand = [
    (lib.getExe cfg.package)
    "--hold-errors"
    configFile
  ];
  # Only the regular and bold faces: the whole Cascadia package weighs ~170 MB.
  cascadiaMono = pkgs.runCommand "cascadia-mono" { } ''
    install -D --mode=444 --target-directory=$out/share/fonts/truetype \
      ${pkgs.cascadia-code}/share/fonts/truetype/CascadiaMono-Regular.ttf \
      ${pkgs.cascadia-code}/share/fonts/truetype/CascadiaMono-Bold.ttf
  '';
  # Started by kmscon as root: hands the token over, then drops privileges.
  kmsconLauncher = pkgs.writeShellApplication {
    name = "polaris-dashboard-session";
    runtimeInputs = [ pkgs.util-linux ];
    text = ''
      ${lib.optionalString (cfg.grafanaTokenFile != null) ''
        if [ -r ${lib.escapeShellArg cfg.grafanaTokenFile} ]; then
          POLARIS_GRAFANA_TOKEN="$(< ${lib.escapeShellArg cfg.grafanaTokenFile})"
          export POLARIS_GRAFANA_TOKEN
        fi
      ''}
      exec setpriv --reuid=${cfg.user} --regid=${cfg.user} --init-groups --no-new-privs -- \
        ${lib.escapeShellArgs dashboardCommand}
    '';
  };
  kmsconArguments = [
    "--vt=${cfg.tty}"
    "--switchvt"
    "--no-issue"
    # Keeps the environment systemd prepared for the launcher
    "--no-reset-env"
    "--no-mouse"
    "--sb-size=0"
    "--dpms-timeout=0"
    "--term=xterm-256color"
    "--font-name=${cfg.font.name}"
    "--font-size=${toString cfg.font.size}"
    "--login"
    "--"
    (lib.getExe kmsconLauncher)
  ];
  gettyUnits = [
    "getty@${cfg.tty}.service"
    "autovt@${cfg.tty}.service"
    "kmsconvt@${cfg.tty}.service"
  ];
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
        It can stay readable by root only. With kmscon a missing file is shown
        on screen; with the linux console the service cannot start without it.
      '';
    };

    console = lib.mkOption {
      type = lib.types.enum [
        "kmscon"
        "linux"
      ];
      default = "kmscon";
      description = ''
        Console drawing the dashboard. `kmscon` renders TrueType fonts and
        displays every chart glyph; `linux` is the kernel console, limited to
        512 glyphs per font (set `tui.chart_marker = "block"` with it).
      '';
    };

    font = {
      name = lib.mkOption {
        type = lib.types.str;
        default = "Cascadia Mono";
        description = "kmscon font family; it must contain braille patterns and block elements.";
      };

      size = lib.mkOption {
        type = lib.types.ints.positive;
        default = 13;
        description = "kmscon font size in pixels; it sets how many cells fit on the screen.";
      };

      packages = lib.mkOption {
        type = lib.types.listOf lib.types.package;
        default = [ cascadiaMono ];
        defaultText = lib.literalExpression "[ cascadia-mono ] # regular and bold faces of pkgs.cascadia-code";
        description = "Font packages made available to kmscon through fontconfig.";
      };
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "polaris-dashboard";
      description = "Unprivileged user running the dashboard.";
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
        Terminus font of the linux console; its size sets the grid resolution
        (ter-v16n on 1280x400 gives 160x25 cells, ter-v24n gives 106x16).
        `null` leaves the console font untouched. Unused with kmscon.
      '';
    };

    videoMode = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "HDMI-A-1:1280x400@60";
      description = "Forced KMS video mode, for screens with a missing or wrong EDID.";
    };
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
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
        # Keep kernel messages from drawing over the dashboard. Stronger than
        # mkDefault, which other profiles use too (the aarch64 SD image sets 7).
        boot.consoleLogLevel = lib.mkOverride 900 3;

        systemd.services."getty@${cfg.tty}".enable = false;

        systemd.services.polaris-dashboard = {
          description = "Polaris console dashboard";
          after = [
            "systemd-user-sessions.service"
            "network-online.target"
          ]
          ++ gettyUnits;
          wants = [ "network-online.target" ];
          conflicts = gettyUnits;
          wantedBy = [ "multi-user.target" ];
          restartTriggers = [ configFile ];
          serviceConfig = {
            Restart = "always";
            RestartSec = 5;
          };
        };
      }

      (lib.mkIf isKmscon {
        fonts.packages = cfg.font.packages;

        systemd.services.polaris-dashboard.serviceConfig = {
          ExecStart = "${lib.getExe' pkgs.kmscon "kmscon"} ${lib.escapeShellArgs kmsconArguments}";
          PrivateTmp = true;
        };
      })

      (lib.mkIf (!isKmscon) {
        console = lib.mkIf (cfg.consoleFont != null) {
          font = cfg.consoleFont;
          packages = [ pkgs.terminus_font ];
          earlySetup = true;
        };

        security.pam.services.polaris-dashboard.text = ''
          auth    required pam_unix.so nullok
          account required pam_unix.so
          session required pam_unix.so
          session required pam_env.so conffile=/etc/pam/environment readenv=0
          session required ${config.systemd.package}/lib/security/pam_systemd.so
        '';

        systemd.services.polaris-dashboard = {
          environment.TERM = "linux";
          serviceConfig = {
            ExecStart = lib.escapeShellArgs dashboardCommand;
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
            LoadCredential = lib.optional (
              cfg.grafanaTokenFile != null
            ) "grafana-token:${cfg.grafanaTokenFile}";
            NoNewPrivileges = true;
            ProtectSystem = "strict";
            ProtectHome = true;
            PrivateTmp = true;
          };
        };
      })
    ]
  );
}
