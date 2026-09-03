{
  self,
}:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.uur;
  defaultPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
in
{
  options.programs.uur = {
    enable = lib.mkEnableOption "UU Remote Linux compatibility layer";
    package = lib.mkOption {
      type = lib.types.package;
      default = defaultPackage;
      defaultText = lib.literalExpression "inputs.uur.packages.${pkgs.system}.default";
      description = "The uur package to install.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    boot.kernelModules = [ "uinput" ];
    services.udev.packages = [ cfg.package ];
  };
}
