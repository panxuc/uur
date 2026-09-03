{
  description = "UU Remote compatibility layer for Linux";

  inputs.nixpkgs.url = "https://channels.nixos.org/nixos-26.05/nixexprs.tar.xz";

  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      uur = pkgs.callPackage ./nix/package.nix { };
    in
    {
      packages.${system} = {
        inherit uur;
        default = uur;
      };

      apps.${system}.default = {
        type = "app";
        program = "${uur}/bin/uur";
        meta.description = "Run the uur command-line interface";
      };

      nixosModules.default = import ./nix/module.nix { inherit self; };

      devShells.${system}.default = pkgs.mkShell {
        inputsFrom = [ uur ];
        packages = [
          pkgs.cargo
          pkgs.rustc
          pkgs.pkgsCross.mingwW64.stdenv.cc
        ];
      };

      formatter.${system} = pkgs.nixfmt;
    };
}
