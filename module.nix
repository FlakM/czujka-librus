{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.services.librus-notifications;
in
{
  options.services.librus-notifications = {
    enable = mkEnableOption "Librus notifications service with AI summarization";

    package = mkOption {
      type = types.package;
      description = "The librus-notifications package to use";
    };

    user = mkOption {
      type = types.str;
      default = "librus-notifications";
      description = "User to run the service as";
    };

    group = mkOption {
      type = types.str;
      default = "librus-notifications";
      description = "Group to run the service as";
    };

    dataDir = mkOption {
      type = types.path;
      default = "/var/lib/librus-notifications";
      description = "Directory to store the SQLite database";
    };

    environmentFile = mkOption {
      type = types.path;
      description = ''
        Path to environment file containing credentials.
        Should contain:
        - LIBRUS_USERNAME
        - LIBRUS_PASSWORD
        - OPENAI_API_KEY
        - SEND_EMAIL (optional)
        - EMAIL_HOST, EMAIL_PORT, EMAIL_USER, EMAIL_PASSWORD, EMAIL_TO (if SEND_EMAIL=true)
        - LOG_LEVEL (optional)
      '';
    };

    schedule = mkOption {
      type = types.listOf types.str;
      default = [ "*-*-* 07:00:00" "*-*-* 15:00:00" ];
      description = "Systemd timer schedule (OnCalendar format)";
      example = [ "daily" "*-*-* 08:00:00" ];
    };

    persistent = mkOption {
      type = types.bool;
      default = true;
      description = "Whether to run missed timers on boot";
    };
  };

  config = mkIf cfg.enable {
    users.users.${cfg.user} = mkIf (cfg.user == "librus-notifications") {
      isSystemUser = true;
      group = cfg.group;
      home = cfg.dataDir;
      createHome = true;
      description = "Librus notifications service user";
    };

    users.groups.${cfg.group} = mkIf (cfg.group == "librus-notifications") {};

    systemd.services.librus-notifications = {
      description = "Librus Notifications Service";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];

      serviceConfig = {
        Type = "oneshot";
        User = cfg.user;
        Group = cfg.group;
        WorkingDirectory = cfg.dataDir;
        ExecStart = "${cfg.package}/bin/librus-notifications";
        EnvironmentFile = cfg.environmentFile;
        Environment = [
          "NODE_ENV=production"
          "DB_PATH=${cfg.dataDir}/librus.db"
        ];
        StandardOutput = "journal";
        StandardError = "journal";
        SyslogIdentifier = "librus-notifications";

        PrivateTmp = true;
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ReadWritePaths = [ cfg.dataDir ];
      };
    };

    systemd.timers.librus-notifications = {
      description = "Librus Notifications Timer";
      wantedBy = [ "timers.target" ];

      timerConfig = {
        OnCalendar = cfg.schedule;
        Persistent = cfg.persistent;
        Unit = "librus-notifications.service";
      };
    };
  };
}
