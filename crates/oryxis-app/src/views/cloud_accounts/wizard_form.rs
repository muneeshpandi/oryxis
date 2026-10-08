//! Add / edit wizard for a `CloudProfile`. Renders a right-side panel
//! with provider + auth pickers and the per-auth-kind input fields,
//! plus a "Test credentials" button and the save / delete actions at
//! the bottom.

use iced::Widget as _;
use iced::border::Radius;
use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input, Row, Space};
use iced::{Background, Border, Color, Element, Length, Padding};

use crate::app::{CloudMessage, NavigationMessage, PluginMessage, Message, Oryxis};
use crate::i18n::t;
use crate::state::{CloudAuthChoice, CloudProviderChoice, CloudTestState};
use crate::theme::OryxisColors;
use crate::widgets::{dir_align_x, dir_row};

impl Oryxis {
    pub(crate) fn view_cloud_form_panel(&self) -> Element<'_, Message> {
        // Keyboard rows are recorded in visual order (row mode: Up/Down from any input).
        self.panel_nav_reset();
        let is_editing = self.cloud_form.editing_id.is_some();
        let title = if is_editing {
            t("cloud_edit_account")
        } else {
            t("cloud_new_account")
        };

        // The close (×) is not a keyboard row: Esc already owns panel
        // close, and recording it would make the header the first Down
        // target instead of the form.
        let panel_header = container(
            dir_row(vec![
                text(title)
                    .size(18)
                    .color(OryxisColors::t().text_primary)
                    .boxed(),
                Space::new().width(Length::Fill).boxed(),
                crate::widgets::panel_close_button(Message::Cloud(CloudMessage::HideCloudForm)).boxed(),
            ])
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding {
            top: 20.0,
            right: 20.0,
            bottom: 16.0,
            left: 20.0,
        });

        // Test Credentials shells out to the provider plugin; if it's
        // not installed, the call would fail with a cryptic
        // `BinaryNotFound` error, so block it at the button level and
        // surface the install banner above. Computed before the form
        // widgets because the banner renders (and records its keyboard
        // row) above them.
        let plugin_missing = !self.is_plugin_ready(self.cloud_form.provider);
        let test_button_disabled =
            matches!(self.cloud_form.test_state, CloudTestState::Running) || plugin_missing;

        // Plugin-missing banner: pinned *above* the scrollable form
        // when the provider chosen above has no installed plugin, so
        // the user can't fill out the form and then hit a cryptic
        // "binary not found" wall on Test Credentials. Every cloud
        // provider (AWS and Kubernetes alike) runs as a subprocess
        // plugin, so both surface this when their plugin is missing.
        // Built before the form fields so the Install button's keyboard
        // row records first, matching its on-screen position.
        let plugin_banner: Element<'_, Message> = if plugin_missing {
            let provider_id_str = self.cloud_form.provider.id();
            // Brand name (not translated) for the title prefix, so the
            // banner reads "AWS plugin not installed" / "Kubernetes
            // plugin not installed" per the selected provider.
            let provider_display = match self.cloud_form.provider {
                CloudProviderChoice::Aws => "AWS",
                CloudProviderChoice::K8s => "Kubernetes",
                CloudProviderChoice::Gcp => "GCP",
                CloudProviderChoice::Azure => "Azure",
                CloudProviderChoice::Aliyun => "Alibaba Cloud",
                CloudProviderChoice::Tencent => "Tencent Cloud",
            };
            let banner_title = format!(
                "{} {}",
                provider_display,
                t("cloud_plugin_missing_title_suffix")
            );
            let install_btn = button(
                container(
                    text(t("plugin_action_install"))
                        .size(12)
                        .color(OryxisColors::t().accent),
                )
                .padding(Padding {
                    top: 6.0,
                    right: 14.0,
                    bottom: 6.0,
                    left: 14.0,
                }),
            )
            .on_press(Message::Plugin(PluginMessage::ShowPluginInstallModal(provider_id_str.to_string())))
            .style(|_, _| button::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().accent,
                    width: 1.0,
                },
                ..Default::default()
            });
            let install_btn = self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Plugin(PluginMessage::ShowPluginInstallModal(
                    provider_id_str.to_string(),
                ))),
                6.0,
                install_btn.boxed(),
            );
            let banner: Element<'_, Message> = container(
                column![
                    dir_row(vec![
                        iced_fonts::lucide::circle_alert()
                            .size(14)
                            .color(OryxisColors::t().warning)
                            .boxed(),
                        Space::new().width(8).boxed(),
                        text(banner_title)
                            .size(13)
                            .color(OryxisColors::t().text_primary)
                            .boxed(),
                    ])
                    .align_y(iced::Alignment::Center),
                    Space::new().height(4),
                    text(t("cloud_plugin_missing_body"))
                        .size(11)
                        .color(OryxisColors::t().text_secondary),
                    Space::new().height(8),
                    container(install_btn)
                        .width(Length::Fill)
                        .align_x(dir_align_x()),
                ]
                .width(Length::Fill),
            )
            .padding(12)
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.10,
                    ..OryxisColors::t().warning
                })),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().warning,
                    width: 1.0,
                },
                ..Default::default()
            })
            .boxed();
            column![banner, Space::new().height(14)].boxed()
        } else {
            Space::new().boxed()
        };

        // Name field, built before the pickers so its keyboard row
        // records first (it's the top field of the form).
        let name_field: Element<'_, Message> = self.panel_nav_slot(
            crate::keynav::RowAction::input(iced::widget::Id::new("panel-cloud-name")),
            10.0,
            text_input("prod-aws", &self.cloud_form.label)
                .id(iced::widget::Id::new("panel-cloud-name"))
                .on_input(|v| Message::Cloud(CloudMessage::CloudFormLabelChanged(v)))
                .padding(10)
                .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                .boxed(),
        );

        // ── Provider picker ── every provider the app knows. Keyboard row:
        // Focusable select: Tab reaches it, Enter/Space open it, the
        // widget owns arrows/Esc while focused (fork support).
        let provider_options = vec![
            CloudProviderChoice::Aws,
            CloudProviderChoice::K8s,
            CloudProviderChoice::Gcp,
            CloudProviderChoice::Azure,
            CloudProviderChoice::Aliyun,
            CloudProviderChoice::Tencent,
        ];
        let provider_pick: Element<'_, Message> = self.panel_nav_slot(
            crate::keynav::RowAction::input(iced::widget::Id::new("cloud-pick-provider")),
            10.0,
            pick_list(
                Some(self.cloud_form.provider),
                provider_options,
                |c| match c {
                    CloudProviderChoice::Aws => "AWS".to_string(),
                    CloudProviderChoice::K8s => "Kubernetes".to_string(),
                    CloudProviderChoice::Gcp => "GCP".to_string(),
                    CloudProviderChoice::Azure => "Azure".to_string(),
                    CloudProviderChoice::Aliyun => "Alibaba Cloud".to_string(),
                    CloudProviderChoice::Tencent => "Tencent Cloud".to_string(),
                },
            )
            .on_select(|v| Message::Cloud(CloudMessage::CloudFormProviderChanged(v)))
            .id(iced::widget::Id::new("cloud-pick-provider"))
            .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
            .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
            .padding(10)
            .text_size(13)
            .style(crate::widgets::rounded_pick_list_style)
            .boxed(),
        );

        // ── Auth picker ── (only Profile is implemented today.)
        let auth_options = match self.cloud_form.provider {
            CloudProviderChoice::Aws => vec![
                CloudAuthChoice::Profile,
                CloudAuthChoice::AccessKey,
                CloudAuthChoice::Sso,
            ],
            CloudProviderChoice::K8s => vec![CloudAuthChoice::Kubeconfig],
            CloudProviderChoice::Gcp => vec![CloudAuthChoice::GcloudCli],
            CloudProviderChoice::Azure => vec![CloudAuthChoice::AzCli],
            CloudProviderChoice::Aliyun => vec![CloudAuthChoice::AliyunCli],
            CloudProviderChoice::Tencent => vec![CloudAuthChoice::TccliCli],
        };
        let auth_pick: Element<'_, Message> = self.panel_nav_slot(
            crate::keynav::RowAction::input(iced::widget::Id::new("cloud-pick-auth")),
            10.0,
            pick_list(
                Some(self.cloud_form.auth_kind),
                auth_options,
                |a| match a {
                    CloudAuthChoice::Profile => t("cloud_auth_profile").to_string(),
                    CloudAuthChoice::AccessKey => t("cloud_auth_access_key").to_string(),
                    CloudAuthChoice::Sso => t("cloud_auth_sso").to_string(),
                    CloudAuthChoice::Kubeconfig => t("cloud_auth_kubeconfig").to_string(),
                    CloudAuthChoice::GcloudCli => t("cloud_auth_gcloud").to_string(),
                    CloudAuthChoice::AzCli => t("cloud_auth_az").to_string(),
                    CloudAuthChoice::AliyunCli => t("cloud_auth_aliyun_cli").to_string(),
                    CloudAuthChoice::TccliCli => t("cloud_auth_tccli").to_string(),
                },
            )
            .on_select(|v| Message::Cloud(CloudMessage::CloudFormAuthKindChanged(v)))
            .id(iced::widget::Id::new("cloud-pick-auth"))
            .on_open(Message::Navigation(NavigationMessage::PickOpenChanged(true)))
            .on_close(Message::Navigation(NavigationMessage::PickOpenChanged(false)))
            .padding(10)
            .text_size(13)
            .style(crate::widgets::rounded_pick_list_style)
            .boxed(),
        );

        // Workload regions, chip list shared across all AWS auth kinds.
        // First chip = default region for single-region API calls; the
        // full list drives discovery fan-out. SSO has its own
        // `sso_region` separately (the IdC endpoint, not workload).
        // Deferred to a closure so its keyboard rows (chip removals +
        // the draft input) record at the arm's position inside the
        // auth fields, keeping recording order equal to visual order.
        let region_field = || {
            let chips: Vec<Element<'_, Message>> = self
                .cloud_form.aws_regions
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    // Chips are a dynamic list: the whole chip records
                    // with its remove action (there is no per-chip
                    // input to focus).
                    self.panel_nav_slot(
                        crate::keynav::RowAction::activate(Message::Cloud(CloudMessage::CloudFormAwsRegionRemove(i))),
                        12.0,
                        region_chip(r.as_str(), i),
                    )
                })
                .collect();
            let chips_block: Element<'_, Message> = if chips.is_empty() {
                Space::new().boxed()
            } else {
                // Plain Row, not dir_row, the chips are content-flow not
                // structural layout and don't need to mirror under RTL.
                container(Row::with_children(chips).spacing(6))
                    .padding(Padding {
                        top: 0.0,
                        right: 0.0,
                        bottom: 6.0,
                        left: 0.0,
                    })
                    .boxed()
            };
            column![
                text(t("cloud_aws_regions"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                chips_block,
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-cloud-region-draft",
                    )),
                    10.0,
                    text_input("us-east-1", &self.cloud_form.aws_region_draft)
                        .id(iced::widget::Id::new("panel-cloud-region-draft"))
                        .on_input(|v| Message::Cloud(CloudMessage::CloudFormAwsRegionDraftChanged(v)))
                        .on_submit(Message::Cloud(CloudMessage::CloudFormAwsRegionAdd))
                        .padding(10)
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x())
                        .boxed(),
                ),
                Space::new().height(4),
                text(t("cloud_aws_regions_hint"))
                    .size(10)
                    .color(OryxisColors::t().text_muted),
            ]
        };

        // Auth-kind-specific fields. We render only the ones that
        // apply to the current pick so the form doesn't sprawl with
        // irrelevant inputs.
        let aws_fields: Element<'_, Message> = match self.cloud_form.auth_kind {
            CloudAuthChoice::Profile => column![
                text(t("cloud_aws_profile_name"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-cloud-aws-profile",
                    )),
                    10.0,
                    text_input("default", &self.cloud_form.aws_profile_name)
                        .id(iced::widget::Id::new("panel-cloud-aws-profile"))
                        .on_input(|v| Message::Cloud(CloudMessage::CloudFormAwsProfileNameChanged(v)))
                        .padding(10)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .boxed(),
                ),
                Space::new().height(14),
                region_field(),
            ].boxed(),
            CloudAuthChoice::AccessKey => {
                let secret_placeholder = if self.cloud_form.aws_has_existing_secret {
                    t("cloud_aws_access_key_secret_kept")
                } else {
                    t("cloud_aws_access_key_secret_ph")
                };
                column![
                    text(t("cloud_aws_access_key_id"))
                        .size(12)
                        .color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    self.panel_nav_slot(
                        crate::keynav::RowAction::input(iced::widget::Id::new(
                            "panel-cloud-aws-key-id",
                        )),
                        10.0,
                        text_input("AKIAIOSFODNN7EXAMPLE", &self.cloud_form.aws_access_key_id)
                            .id(iced::widget::Id::new("panel-cloud-aws-key-id"))
                            .on_input(|v| Message::Cloud(CloudMessage::CloudFormAwsAccessKeyIdChanged(v)))
                            .padding(10)
                            .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                            .boxed(),
                    ),
                    Space::new().height(14),
                    text(t("cloud_aws_access_key_secret"))
                        .size(12)
                        .color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    // Keyboard rows: the field, then its reveal eye (#52).
                    {
                        self.panel_nav_record(crate::keynav::RowAction::input(
                            iced::widget::Id::new("panel-cloud-aws-key-secret"),
                        ));
                        crate::widgets::password_input_with_eye_nav(
                            secret_placeholder,
                            &self.cloud_form.aws_access_key_secret,
                            |v| Message::Cloud(CloudMessage::CloudFormAwsAccessKeySecretChanged(v.into())),
                            None,
                            self.cloud_form.aws_access_key_secret_visible,
                            Message::Cloud(CloudMessage::CloudFormAwsAccessKeySecretToggleVisibility),
                            10.0,
                            Some(iced::widget::Id::new("panel-cloud-aws-key-secret")),
                            |eye| {
                                self.panel_nav_slot(
                                    crate::keynav::RowAction::activate(
                                        Message::Cloud(CloudMessage::CloudFormAwsAccessKeySecretToggleVisibility),
                                    ),
                                    6.0,
                                    eye,
                                )
                            },
                        )
                    },
                    Space::new().height(14),
                    text(t("cloud_aws_access_key_session_token"))
                        .size(12)
                        .color(OryxisColors::t().text_secondary),
                    Space::new().height(4),
                    self.panel_nav_slot(
                        crate::keynav::RowAction::input(iced::widget::Id::new(
                            "panel-cloud-aws-session-token",
                        )),
                        10.0,
                        text_input(t("cloud_aws_access_key_session_token_ph"), &self.cloud_form.aws_access_key_session_token)
                            .id(iced::widget::Id::new("panel-cloud-aws-session-token"))
                            .on_input(|v| Message::Cloud(CloudMessage::CloudFormAwsAccessKeySessionTokenChanged(v.into())))
                            .padding(10)
                            .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                            .boxed(),
                    ),
                    Space::new().height(14),
                    region_field(),
                ].boxed()
            }
            CloudAuthChoice::Sso => column![
                text(t("cloud_aws_sso_start_url"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-cloud-sso-start-url",
                    )),
                    10.0,
                    text_input("https://acme.awsapps.com/start", &self.cloud_form.aws_sso_start_url)
                        .id(iced::widget::Id::new("panel-cloud-sso-start-url"))
                        .on_input(|v| Message::Cloud(CloudMessage::CloudFormAwsSsoStartUrlChanged(v)))
                        .padding(10)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .boxed(),
                ),
                Space::new().height(14),
                text(t("cloud_aws_sso_region"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-cloud-sso-region",
                    )),
                    10.0,
                    text_input("us-east-1", &self.cloud_form.aws_sso_region)
                        .id(iced::widget::Id::new("panel-cloud-sso-region"))
                        .on_input(|v| Message::Cloud(CloudMessage::CloudFormAwsSsoRegionChanged(v)))
                        .padding(10)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .boxed(),
                ),
                Space::new().height(14),
                text(t("cloud_aws_sso_account_id"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-cloud-sso-account",
                    )),
                    10.0,
                    text_input("123456789012", &self.cloud_form.aws_sso_account_id)
                        .id(iced::widget::Id::new("panel-cloud-sso-account"))
                        .on_input(|v| Message::Cloud(CloudMessage::CloudFormAwsSsoAccountIdChanged(v)))
                        .padding(10)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .boxed(),
                ),
                Space::new().height(14),
                text(t("cloud_aws_sso_role_name"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-cloud-sso-role",
                    )),
                    10.0,
                    text_input("AdministratorAccess", &self.cloud_form.aws_sso_role_name)
                        .id(iced::widget::Id::new("panel-cloud-sso-role"))
                        .on_input(|v| Message::Cloud(CloudMessage::CloudFormAwsSsoRoleNameChanged(v)))
                        .padding(10)
                        .style(crate::widgets::rounded_input_style).align_x(dir_align_x())
                        .boxed(),
                ),
                Space::new().height(14),
                region_field(),
                Space::new().height(8),
                text(t("cloud_aws_sso_hint"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
            ].boxed(),
            CloudAuthChoice::Kubeconfig => column![
                text(t("cloud_k8s_kubeconfig_path"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-cloud-kubeconfig-path",
                    )),
                    10.0,
                    text_input(t("cloud_k8s_kubeconfig_ph"), &self.cloud_form.kubeconfig_path)
                        .id(iced::widget::Id::new("panel-cloud-kubeconfig-path"))
                        .on_input(|v| Message::Cloud(CloudMessage::CloudFormKubeconfigPathChanged(v)))
                        .padding(10)
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x())
                        .boxed(),
                ),
                Space::new().height(4),
                text(t("cloud_k8s_kubeconfig_hint"))
                    .size(10)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(14),
                text(t("cloud_k8s_context"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-cloud-k8s-context",
                    )),
                    10.0,
                    text_input(t("cloud_k8s_context_ph"), &self.cloud_form.context)
                        .id(iced::widget::Id::new("panel-cloud-k8s-context"))
                        .on_input(|v| Message::Cloud(CloudMessage::CloudFormContextChanged(v)))
                        .padding(10)
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x())
                        .boxed(),
                ),
                Space::new().height(4),
                text(t("cloud_k8s_context_hint"))
                    .size(10)
                    .color(OryxisColors::t().text_muted),
            ].boxed(),
            CloudAuthChoice::GcloudCli => column![
                // GCP uses the ambient gcloud login; no secret here, just
                // an optional project scope.
                text(t("cloud_gcp_login_hint"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(14),
                text(t("cloud_gcp_project"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-cloud-gcp-project",
                    )),
                    10.0,
                    text_input(t("cloud_gcp_project_ph"), &self.cloud_form.gcp_project)
                        .id(iced::widget::Id::new("panel-cloud-gcp-project"))
                        .on_input(|v| Message::Cloud(CloudMessage::CloudFormGcpProjectChanged(v)))
                        .padding(10)
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x())
                        .boxed(),
                ),
                Space::new().height(4),
                text(t("cloud_gcp_project_hint"))
                    .size(10)
                    .color(OryxisColors::t().text_muted),
            ].boxed(),
            CloudAuthChoice::AzCli => column![
                // Azure uses the ambient az login; no secret here, just an
                // optional subscription scope.
                text(t("cloud_azure_login_hint"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(14),
                text(t("cloud_azure_subscription"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-cloud-azure-subscription",
                    )),
                    10.0,
                    text_input(
                        t("cloud_azure_subscription_ph"),
                        &self.cloud_form.azure_subscription,
                    )
                    .id(iced::widget::Id::new("panel-cloud-azure-subscription"))
                    .on_input(|v| Message::Cloud(CloudMessage::CloudFormAzureSubscriptionChanged(v)))
                    .padding(10)
                    .style(crate::widgets::rounded_input_style)
                    .align_x(dir_align_x())
                    .boxed(),
                ),
                Space::new().height(4),
                text(t("cloud_azure_subscription_hint"))
                    .size(10)
                    .color(OryxisColors::t().text_muted),
            ].boxed(),
            CloudAuthChoice::AliyunCli => column![
                // Alibaba Cloud uses the CLI's own configured profiles; no secret
                // here, just an optional profile + region scope.
                text(t("cloud_aliyun_login_hint"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(14),
                text(t("cloud_cli_profile"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-cloud-cli-profile",
                    )),
                    10.0,
                    text_input(t("cloud_cli_profile_ph"), &self.cloud_form.cli_profile)
                        .id(iced::widget::Id::new("panel-cloud-cli-profile"))
                        .on_input(|v| Message::Cloud(CloudMessage::CloudFormCliProfileChanged(v)))
                        .padding(10)
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x())
                        .boxed(),
                ),
                Space::new().height(4),
                text(t("cloud_cli_profile_hint"))
                    .size(10)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(14),
                text(t("cloud_cli_region"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-cloud-cli-region",
                    )),
                    10.0,
                    text_input(t("cloud_aliyun_region_ph"), &self.cloud_form.cli_region)
                        .id(iced::widget::Id::new("panel-cloud-cli-region"))
                        .on_input(|v| Message::Cloud(CloudMessage::CloudFormCliRegionChanged(v)))
                        .padding(10)
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x())
                        .boxed(),
                ),
                Space::new().height(4),
                text(t("cloud_cli_region_hint"))
                    .size(10)
                    .color(OryxisColors::t().text_muted),
            ].boxed(),
            CloudAuthChoice::TccliCli => column![
                // Tencent Cloud uses the CLI's own configured profiles; no secret
                // here, just an optional profile + region scope.
                text(t("cloud_tencent_login_hint"))
                    .size(11)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(14),
                text(t("cloud_cli_profile"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-cloud-cli-profile",
                    )),
                    10.0,
                    text_input(t("cloud_cli_profile_ph"), &self.cloud_form.cli_profile)
                        .id(iced::widget::Id::new("panel-cloud-cli-profile"))
                        .on_input(|v| Message::Cloud(CloudMessage::CloudFormCliProfileChanged(v)))
                        .padding(10)
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x())
                        .boxed(),
                ),
                Space::new().height(4),
                text(t("cloud_cli_profile_hint"))
                    .size(10)
                    .color(OryxisColors::t().text_muted),
                Space::new().height(14),
                text(t("cloud_cli_region"))
                    .size(12)
                    .color(OryxisColors::t().text_secondary),
                Space::new().height(4),
                self.panel_nav_slot(
                    crate::keynav::RowAction::input(iced::widget::Id::new(
                        "panel-cloud-cli-region",
                    )),
                    10.0,
                    text_input(t("cloud_tencent_region_ph"), &self.cloud_form.cli_region)
                        .id(iced::widget::Id::new("panel-cloud-cli-region"))
                        .on_input(|v| Message::Cloud(CloudMessage::CloudFormCliRegionChanged(v)))
                        .padding(10)
                        .style(crate::widgets::rounded_input_style)
                        .align_x(dir_align_x())
                        .boxed(),
                ),
                Space::new().height(4),
                text(t("cloud_cli_region_hint"))
                    .size(10)
                    .color(OryxisColors::t().text_muted),
            ].boxed(),
        };

        // ── Test credentials button + result line ──
        let test_status: Element<'_, Message> = match &self.cloud_form.test_state {
            CloudTestState::Idle => Space::new().boxed(),
            CloudTestState::Running => text(t("cloud_test_running"))
                .size(11)
                .color(OryxisColors::t().text_muted)
                .boxed(),
            CloudTestState::Ok => text(t("cloud_test_ok"))
                .size(11)
                .color(OryxisColors::t().success)
                .boxed(),
            CloudTestState::Failed(msg) => {
                text(format!("{}: {msg}", t("cloud_test_failed")))
                    .size(11)
                    .color(OryxisColors::t().error)
                    .boxed()
            }
        };

        let test_btn = {
            let mut btn = button(
                container(
                    text(t("cloud_test_credentials"))
                        .size(13)
                        .color(OryxisColors::t().text_primary),
                )
                .padding(Padding {
                    top: 8.0,
                    right: 0.0,
                    bottom: 8.0,
                    left: 0.0,
                })
                .width(Length::Fill)
                .center_x(Length::Fill),
            )
            .width(Length::Fill)
            .style(|_, _| button::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(8.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            });
            if !test_button_disabled {
                btn = btn.on_press(Message::Cloud(CloudMessage::CloudFormTestCredentials));
            }
            btn
        };
        // Recorded only when pressable: while a test runs (or the
        // plugin is missing) the button has no on_press, so there is
        // nothing for Enter to fire.
        let test_btn: Element<'_, Message> = if test_button_disabled {
            test_btn.boxed()
        } else {
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Cloud(CloudMessage::CloudFormTestCredentials)),
                8.0,
                test_btn.boxed(),
            )
        };

        let form = column![
            text(t("name"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            name_field,
            Space::new().height(14),
            text(t("cloud_provider"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            provider_pick,
            Space::new().height(14),
            text(t("cloud_auth_method"))
                .size(12)
                .color(OryxisColors::t().text_secondary),
            Space::new().height(4),
            auth_pick,
            Space::new().height(14),
            aws_fields,
            Space::new().height(16),
            test_btn,
            Space::new().height(6),
            test_status,
        ]
        .width(Length::Fill)
        .align_x(dir_align_x());

        // Shared form chrome: inline error + Cancel/Save footer. The
        // Delete row (editing only) keeps its outlined-danger style in
        // the body, above the footer.
        let panel_error = crate::widgets::form_error(self.cloud_form.error.as_deref());
        let footer = crate::widgets::form_footer(
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Cloud(CloudMessage::HideCloudForm)),
                6.0,
                crate::widgets::form_cancel_button(Message::Cloud(CloudMessage::HideCloudForm)),
            ),
            self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Cloud(CloudMessage::SaveCloudProfile)),
                6.0,
                crate::widgets::form_save_button(t("save"), Some(Message::Cloud(CloudMessage::SaveCloudProfile))),
            ),
        );

        let mut bottom = iced::widget::Column::<iced::Element<'_, _>>::new();
        if let Some(edit_id) = self.cloud_form.editing_id {
            let del_btn = self.panel_nav_slot(
                crate::keynav::RowAction::activate(Message::Cloud(CloudMessage::DeleteCloudProfile(edit_id))),
                8.0,
                button(
                    container(
                        text(t("delete"))
                            .size(13)
                            .color(OryxisColors::t().error),
                    )
                    .padding(Padding {
                        top: 10.0,
                        right: 0.0,
                        bottom: 10.0,
                        left: 0.0,
                    })
                    .width(Length::Fill)
                    .center_x(Length::Fill),
                )
                .on_press(Message::Cloud(CloudMessage::DeleteCloudProfile(edit_id)))
                .width(Length::Fill)
                .style(|_, _| button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    border: Border {
                        radius: Radius::from(8.0),
                        color: OryxisColors::t().error,
                        width: 1.0,
                    },
                    ..Default::default()
                })
                .boxed(),
            );
            bottom = bottom.push(Space::new().height(8).boxed());
            bottom = bottom.push(del_btn);
        }

        let panel_content = column![
            panel_header,
            container(
                column![
                    // Pinned above the scroll so the install affordance
                    // stays visible while the user scrolls the fields.
                    plugin_banner,
                    scrollable(form)
                        // Shared id: the keyboard router keeps the
                        // selected row in view.
                        .id(iced::widget::Id::new("side-panel-scroll"))
                        .height(Length::Fill),
                    Space::new().height(8),
                    bottom,
                ]
                .height(Length::Fill)
                .width(Length::Fill)
                .align_x(dir_align_x()),
            )
            .padding(Padding {
                top: 0.0,
                right: 20.0,
                bottom: 0.0,
                left: 20.0,
            })
            .height(Length::Fill),
            panel_error,
            footer,
        ]
        .height(Length::Fill);

        // Standardised side-panel chrome (matches host editor,
        // discovery, dynamic-group editor) so every right-panel
        // editor shares the same background surface.
        crate::widgets::side_panel_frame(panel_content.boxed(), OryxisColors::t().bg_surface, self.panel_width)
    }
}

fn region_chip(label: &str, idx: usize) -> Element<'_, Message> {
    let accent = OryxisColors::t().accent;
    container(
        row![
            text(label.to_string())
                .size(11)
                .color(OryxisColors::t().text_primary),
            Space::new().width(2),
            button(
                text("\u{00D7}")
                    .size(13)
                    .color(OryxisColors::t().text_muted),
            )
            .padding(Padding {
                top: 0.0,
                right: 6.0,
                bottom: 0.0,
                left: 6.0,
            })
            .on_press(Message::Cloud(CloudMessage::CloudFormAwsRegionRemove(idx)))
            .style(|_, _| button::Style {
                background: None,
                ..Default::default()
            }),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding {
        top: 2.0,
        right: 0.0,
        bottom: 2.0,
        left: 10.0,
    })
    .style(move |_| container::Style {
        background: Some(Background::Color(Color {
            a: 0.12,
            ..accent
        })),
        border: Border {
            radius: Radius::from(12.0),
            color: Color { a: 0.30, ..accent },
            width: 1.0,
        },
        ..Default::default()
    })
    .boxed()
}
