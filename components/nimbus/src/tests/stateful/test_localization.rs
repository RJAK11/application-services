/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::enrollment::{
    DisqualifiedReason, EnrollmentsEvolver, NotEnrolledReason, Participation,
    map_features_by_feature_id,
};
use crate::stateful::behavior::EventStore;

use crate::evaluator::evaluate_enrollment;
use crate::tests::helpers::{get_single_feature_experiment, no_coenrolling_features};
use crate::{
    AppContext, AvailableRandomizationUnits, NimbusTargetingHelper, Result, TargetingAttributes,
};
use crate::{EnrollmentStatus, Experiment};

fn get_localization_bundle() -> Value {
    return json!({
        "en": {
            "text-id": "English text",
            "label-id": "English label"
        },
        "fr": {
            "text-id": "French text",
            "label-id": "French label"
        }
    });
}

fn get_l10n_experiment(localizations: Value) -> Experiment {
    let mut exp = get_single_feature_experiment(
        "l10n-experiment",
        "feature-id",
        json!({
            "title": "test",
            "text": {
                "$l10n": { "id": "text-id", "text": "test text", "comment": "test comment 1" }
            },
            "items": [
                { "label": { "$l10n": { "id": "label-id", "text": "Label", "comment": "test comment 2" } } }
            ],

        }),
    );
    exp.localizations = serde_json::from_value(localizations).unwrap();
    return exp;
}

fn get_targeting_helper(locale: Option<&str>) -> NimbusTargetingHelper {
    let targeting_attributes: TargetingAttributes = AppContext {
        app_name: "fenix".to_string(),
        app_id: "org.mozilla.fenix".to_string(),
        channel: "nightly".to_string(),
        locale: locale.map(|l| l.to_string()),
        ..Default::default()
    }
    .into();
    return NimbusTargetingHelper::new(
        targeting_attributes,
        Arc::new(Mutex::new(EventStore::new())),
        None,
    );
}

#[test]
fn test_l10n_enrolls_when_language_present() -> Result<()> {
    let experiment = get_l10n_experiment(get_localization_bundle());
    let targeting_helper = get_targeting_helper(Some("fr-CA"));
    let enrollment = evaluate_enrollment(
        &AvailableRandomizationUnits::with_nimbus_id(&uuid::Uuid::new_v4()),
        &experiment,
        &targeting_helper,
    )
    .unwrap();
    assert!(matches!(
        enrollment.status,
        EnrollmentStatus::Enrolled { .. }
    ));
    Ok(())
}

#[test]
fn test_l10n_when_language_missing() -> Result<()> {
    let experiment = get_l10n_experiment(get_localization_bundle());
    for locale in [Some("de-DE"), None] {
        let targeting_helper = get_targeting_helper(locale);
        let enrollment = evaluate_enrollment(
            &AvailableRandomizationUnits::with_nimbus_id(&uuid::Uuid::new_v4()),
            &experiment,
            &targeting_helper,
        )
        .unwrap();
        assert!(matches!(
            enrollment.status,
            EnrollmentStatus::NotEnrolled {
                reason: NotEnrolledReason::L10nMissingLanguage
            }
        ));
    }
    Ok(())
}

#[test]
fn test_l10n_when_entry_missing() -> Result<()> {
    let experiment_1 = get_l10n_experiment(json!({
        "fr": {
            "label-id": "French label"
        }
    }));
    let experiment_2 = get_l10n_experiment(json!({
        "fr": {
            "text-id": "French text"
        }
    }));
    let targeting_helper = get_targeting_helper(Some("fr-CA"));
    for experiment in [experiment_1, experiment_2] {
        let enrollment = evaluate_enrollment(
            &AvailableRandomizationUnits::with_nimbus_id(&uuid::Uuid::new_v4()),
            &experiment,
            &targeting_helper,
        )
        .unwrap();
        assert!(matches!(
            enrollment.status,
            EnrollmentStatus::NotEnrolled {
                reason: NotEnrolledReason::L10nMissingEntry
            }
        ));
    }
    Ok(())
}

#[test]
fn test_no_l10n_unaffected() -> Result<()> {
    let experiment = get_single_feature_experiment("no-l10n-experiment", "feature-id", json!({}));
    let targeting_helper = get_targeting_helper(Some("fr-CA"));
    let enrollment = evaluate_enrollment(
        &AvailableRandomizationUnits::with_nimbus_id(&uuid::Uuid::new_v4()),
        &experiment,
        &targeting_helper,
    )
    .unwrap();
    assert!(matches!(
        enrollment.status,
        EnrollmentStatus::Enrolled { .. }
    ));
    Ok(())
}

#[test]
fn test_l10n_substitution_in_feature_map() -> Result<()> {
    let experiment = get_l10n_experiment(get_localization_bundle());
    let cases = [
        (
            "fr-CA",
            "fr",
            json!({
                "title": "test",
                "text": "French text",
                "items": [{"label": "French label"}]
            }),
        ),
        (
            "en-CA",
            "en",
            json!({
                "title": "test",
                "text": "English text",
                "items": [{"label": "English label"}]
            }),
        ),
    ];

    for (locale, language, expected) in cases {
        let targeting_helper = get_targeting_helper(Some(locale));
        let enrollment = evaluate_enrollment(
            &AvailableRandomizationUnits::with_nimbus_id(&uuid::Uuid::new_v4()),
            &experiment,
            &targeting_helper,
        )
        .unwrap();
        let features = map_features_by_feature_id(
            &[enrollment],
            std::slice::from_ref(&experiment),
            &no_coenrolling_features(),
            Some(language),
        );
        let value = &features["feature-id"].feature.value;
        assert_eq!(Value::Object(value.clone()), expected);
    }
    Ok(())
}

#[test]
fn test_l10n_disqualifies_when_language_no_longer_present() -> Result<()> {
    let experiment = get_l10n_experiment(get_localization_bundle());
    let mut targeting_helper = get_targeting_helper(Some("fr-CA"));
    let aru = AvailableRandomizationUnits::with_nimbus_id(&uuid::Uuid::new_v4());
    let ids = no_coenrolling_features();
    let mut evolver = EnrollmentsEvolver::new(&aru, &mut targeting_helper, &ids);

    let (enrollments, _) = evolver.evolve_enrollments(
        Participation::default(),
        &[],
        &[experiment.clone()],
        &[],
        None,
    )?;
    assert!(matches!(
        enrollments[0].status,
        EnrollmentStatus::Enrolled { .. }
    ));

    let (enrollments_1, events) = evolver.evolve_enrollments(
        Participation::default(),
        &[experiment.clone()],
        &[get_l10n_experiment(
            json!({"en": {"text-id": "English text","label-id": "English label"}}),
        )],
        &enrollments,
        None,
    )?;
    let enr_1 = enrollments_1.first().unwrap();
    assert!(matches!(
        &enr_1.status,
        EnrollmentStatus::Disqualified {
            reason: DisqualifiedReason::L10nMissingLanguage,
            ..
        }
    ));
    assert_eq!(events.len(), 1);

    let (enrollments_2, events) = evolver.evolve_enrollments(
        Participation::default(),
        &[experiment.clone()],
        &[get_l10n_experiment(
            json!({"fr": {"label-id": "French label"}}),
        )],
        &enrollments,
        None,
    )?;
    let enr_2 = enrollments_2.first().unwrap();
    assert!(matches!(
        &enr_2.status,
        EnrollmentStatus::Disqualified {
            reason: DisqualifiedReason::L10nMissingEntry,
            ..
        }
    ));
    assert_eq!(events.len(), 1);

    Ok(())
}
