use super::*;
use serde_json::json;
use std::path::PathBuf;

const HTTP: &str = concat!("http://fixture:", "example", "@127.0.0.1:18080/");
const SS: &str = concat!("ss://aes-128-gcm:", "example", "@127.0.0.1:18081/");

struct Legacy {
    root: PathBuf,
    workspace: Workspace,
    source_id: Uuid,
    old_id: Uuid,
}

impl Legacy {
    fn new(urls: &[&str]) -> Self {
        let root = std::env::temp_dir().join(format!("muse-refresh-{}", Uuid::new_v4()));
        let store = ProtectedStore::new(&root);
        let mut workspace = Workspace::new();
        let account = workspace.add_account("Fixture");
        let mut sub = ProxySubscription {
            id: Uuid::new_v4(),
            revision: Uuid::new_v4(),
            name: "Local".into(),
            nodes: vec![],
        };
        let mut data = PrivateData {
            url: "http://127.0.0.1/feed".into(),
            labels: HashMap::new(),
            reasons: HashMap::new(),
        };
        for (index, url) in urls.iter().enumerate() {
            let id = Uuid::new_v4();
            let rev = Uuid::new_v4();
            let label = format!("Node {index}");
            store.save_revision(id, rev, url).unwrap();
            data.labels.insert(id, label.clone());
            sub.nodes.push(SubscriptionNode {
                id,
                revision: Some(rev),
                name: label,
                in_pool: None,
                protocol: url.split(':').next().unwrap().into(),
                reason: None,
            });
        }
        store
            .save_protected(
                sub.id,
                Some(sub.revision),
                &serde_json::to_string(&data).unwrap(),
            )
            .unwrap();
        let old_id = sub.nodes[0].id;
        workspace
            .update_network(
                account,
                AccountNetwork::SubscriptionNode {
                    node_id: old_id,
                    force_proxy: true,
                },
                None,
            )
            .unwrap();
        let source_id = sub.id;
        workspace.subscriptions.push(sub);
        crate::proxy_pool::migrate(&root, &mut workspace).unwrap();
        Self {
            root,
            workspace,
            source_id,
            old_id,
        }
    }

    fn apply(&mut self, existing: Option<Uuid>, nodes: &[Value]) {
        let entries = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| Entry {
                label: format!("Node {index}"),
                kind: node["type"].as_str().unwrap().into(),
                node: Some(node.clone()),
                reason: None,
            })
            .collect();
        replace(
            &self.root,
            &mut self.workspace,
            existing,
            "Local",
            "http://127.0.0.1/feed",
            entries,
        )
        .unwrap();
    }

    fn bound_revision(&self) -> Option<Uuid> {
        let account = &self.workspace.accounts[0];
        assert_eq!(
            account.network,
            AccountNetwork::SubscriptionNode {
                node_id: self.old_id,
                force_proxy: true
            }
        );
        self.workspace
            .proxy_credentials(account)
            .ok()
            .and_then(|(id, rev)| {
                assert_eq!(id, self.old_id);
                rev
            })
    }
}

impl Drop for Legacy {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn first_refresh_preserves_unique_legacy_defaults_and_persists_native_revisions() {
    for (url, field) in [(HTTP, "tls"), (SS, "udp")] {
        let mut fixture = Legacy::new(&[url]);
        let previous = fixture.bound_revision().unwrap();
        let mut node = mihomo::snapshot(url).unwrap();
        node[field] = json!(false);
        fixture.apply(Some(fixture.source_id), &[node.clone()]);
        let migrated = fixture
            .bound_revision()
            .expect("unchanged route must remain bound");
        assert_ne!(migrated, previous);
        assert!(fixture.workspace.subscriptions[0].nodes[0].selectable());
        let store = ProtectedStore::new(&fixture.root);
        assert!(store.load_selected(fixture.old_id, Some(previous)).is_err());
        assert_eq!(
            store
                .load_selected(fixture.old_id, Some(migrated))
                .unwrap()
                .as_str(),
            encoded(node.clone()).as_str()
        );
        fixture.apply(Some(fixture.source_id), &[node]);
        assert_eq!(fixture.bound_revision(), Some(migrated));
    }
}

#[test]
fn first_refresh_rejects_ambiguous_or_changed_routes_even_when_labels_match() {
    for (url, field) in [(HTTP, "tls"), (SS, "udp")] {
        let plain = mihomo::snapshot(url).unwrap();
        let mut explicit = plain.clone();
        explicit[field] = json!(false);
        let mut changed_auth = explicit.clone();
        changed_auth["password"] = json!("different");
        let mut enabled = explicit.clone();
        enabled[field] = json!(true);
        let mut changed_port = explicit.clone();
        changed_port["port"] = json!(19001);
        for (old, new) in [
            (vec![url], vec![explicit.clone(), plain.clone()]),
            (vec![url, url], vec![plain.clone()]),
            (vec![url], vec![changed_auth]),
            (vec![url], vec![enabled]),
            (vec![url], vec![changed_port]),
        ] {
            let mut fixture = Legacy::new(&old);
            fixture.apply(Some(fixture.source_id), &new);
            assert_eq!(fixture.bound_revision(), None, "ambiguous or changed route");
            assert!(
                fixture.workspace.subscriptions[0]
                    .nodes
                    .iter()
                    .all(|node| !node.selectable())
            );
        }
        let mut fixture = Legacy::new(&[url]);
        let original = fixture.bound_revision();
        fixture.apply(None, &[explicit]);
        assert_eq!(fixture.bound_revision(), original);
        assert_ne!(
            fixture.workspace.subscriptions[1].nodes[0].id,
            fixture.old_id
        );
        assert!(
            replace(
                &fixture.root,
                &mut fixture.workspace,
                Some(fixture.source_id),
                "Local",
                "http://127.0.0.1/other",
                vec![]
            )
            .is_err()
        );
        assert_eq!(fixture.bound_revision(), original);
    }
}

#[test]
fn first_refresh_rejects_a_collision_between_native_and_legacy_records() {
    let mut explicit = mihomo::snapshot(HTTP).unwrap();
    explicit["tls"] = json!(false);
    let native = encoded(explicit.clone());
    let mut fixture = Legacy::new(&[HTTP, &native]);
    let old_ids: Vec<_> = fixture.workspace.subscriptions[0]
        .nodes
        .iter()
        .map(|node| node.id)
        .collect();
    fixture.apply(Some(fixture.source_id), &[explicit]);
    assert!(fixture.bound_revision().is_none());
    assert!(!old_ids.contains(&fixture.workspace.subscriptions[0].nodes[0].id));
}
