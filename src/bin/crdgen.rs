//! Prints the four glitchtip.ruck.io CRDs as multi-document YAML.
//! Foreign CRDs (pgop.ruck.io, gateway.networking.k8s.io) are consumed, not
//! owned, and must never be emitted here.

use kube::CustomResourceExt;

use glitchtip_operator::crds::{GlitchTip, GlitchTipOrganization, GlitchTipProject, GlitchTipTeam};

fn main() {
    let crds = [
        GlitchTip::crd(),
        GlitchTipOrganization::crd(),
        GlitchTipTeam::crd(),
        GlitchTipProject::crd(),
    ];
    for crd in crds {
        println!("---");
        print!("{}", serde_yaml::to_string(&crd).expect("serializable CRD"));
    }
}
