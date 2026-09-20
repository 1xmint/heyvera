-- HeyVera Socials schema baseline. Generated from the final mixed v68 schema.
-- Domain tables are restricted to SOCIAL_TABLES in the heyvera-db crate.

CREATE TABLE schema_migrations (
id TEXT PRIMARY KEY,
checksum TEXT NOT NULL,
applied_at INTEGER NOT NULL
) WITHOUT ROWID;

-- table subscriptions on subscriptions
CREATE TABLE subscriptions (
            clerk_user_id TEXT PRIMARY KEY,
            stripe_customer_id TEXT NOT NULL,
            stripe_subscription_id TEXT,
            plan_type TEXT NOT NULL DEFAULT 'monthly',
            status TEXT NOT NULL DEFAULT 'trialing',
            trial_end TEXT,
            current_period_start TEXT,
            current_period_end TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- table billing_history on billing_history
CREATE TABLE billing_history (
            id TEXT PRIMARY KEY,
            clerk_user_id TEXT NOT NULL,
            stripe_event_id TEXT UNIQUE,
            amount_cents INTEGER NOT NULL,
            description TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- table referral_codes on referral_codes
CREATE TABLE referral_codes (
            code TEXT PRIMARY KEY,
            creator_user_id TEXT NOT NULL,
            uses_remaining INTEGER NOT NULL DEFAULT 1,
            total_uses INTEGER NOT NULL DEFAULT 0,
            credits_earned REAL NOT NULL DEFAULT 0.0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        , weeks_earned INTEGER NOT NULL DEFAULT 0, max_uses INTEGER NOT NULL DEFAULT 50);

-- table promo_codes on promo_codes
CREATE TABLE promo_codes (
            id TEXT PRIMARY KEY,
            code TEXT NOT NULL UNIQUE COLLATE NOCASE,
            discount_type TEXT NOT NULL DEFAULT 'trial_extension',
            discount_value REAL NOT NULL DEFAULT 14.0,
            max_uses INTEGER NOT NULL DEFAULT 25,
            current_uses INTEGER NOT NULL DEFAULT 0,
            expires_at TEXT,
            active INTEGER NOT NULL DEFAULT 1,
            created_by TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            description TEXT
        , discount_options TEXT);

-- table code_redemptions on code_redemptions
CREATE TABLE code_redemptions (
            id TEXT PRIMARY KEY,
            promo_code_id TEXT NOT NULL REFERENCES promo_codes(id),
            code TEXT NOT NULL,
            user_id TEXT NOT NULL,
            redeemed_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(code, user_id)
        );

-- index idx_code_redemptions_user on code_redemptions
CREATE INDEX idx_code_redemptions_user ON code_redemptions(user_id);

-- index idx_code_redemptions_code on code_redemptions
CREATE INDEX idx_code_redemptions_code ON code_redemptions(code);

-- table social_profiles on social_profiles
CREATE TABLE social_profiles (
            id TEXT PRIMARY KEY, clerk_user_id TEXT NOT NULL, handle TEXT NOT NULL,
            display_name TEXT NOT NULL, bio TEXT NOT NULL DEFAULT '', avatar_url TEXT,
            banner_url TEXT, location TEXT, website_url TEXT,
            proof_state TEXT NOT NULL DEFAULT 'pending', continuity_state TEXT NOT NULL DEFAULT 'pending',
            created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_social_profiles_clerk on social_profiles
CREATE UNIQUE INDEX idx_social_profiles_clerk ON social_profiles(clerk_user_id);

-- index idx_social_profiles_handle on social_profiles
CREATE UNIQUE INDEX idx_social_profiles_handle ON social_profiles(handle);

-- table social_linked_agents on social_linked_agents
CREATE TABLE social_linked_agents (
            id TEXT PRIMARY KEY, profile_id TEXT NOT NULL REFERENCES social_profiles(id),
            agent_name TEXT NOT NULL, agent_slug TEXT NOT NULL, agent_key TEXT NOT NULL,
            agent_key_hash TEXT,
            agent_type TEXT NOT NULL DEFAULT 'general', link_state TEXT NOT NULL DEFAULT 'active',
            visibility TEXT NOT NULL DEFAULT 'public', proof_state TEXT NOT NULL DEFAULT 'pending',
            is_primary INTEGER NOT NULL DEFAULT 0,
            auto_reply_enabled INTEGER NOT NULL DEFAULT 0,
            auto_follow_enabled INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_social_linked_agents_profile on social_linked_agents
CREATE INDEX idx_social_linked_agents_profile ON social_linked_agents(profile_id);

-- index idx_social_linked_agents_key_hash on social_linked_agents
CREATE UNIQUE INDEX idx_social_linked_agents_key_hash
            ON social_linked_agents(agent_key_hash)
            WHERE agent_key_hash IS NOT NULL AND agent_key_hash != '';

-- table social_posts on social_posts
CREATE TABLE social_posts (
            id TEXT PRIMARY KEY, profile_id TEXT NOT NULL REFERENCES social_profiles(id),
            linked_agent_id TEXT REFERENCES social_linked_agents(id), body TEXT NOT NULL,
            visibility TEXT NOT NULL DEFAULT 'public', proof_state TEXT NOT NULL DEFAULT 'pending',
            author_mode TEXT NOT NULL DEFAULT 'person',
            reply_to_post_id TEXT REFERENCES social_posts(id), quote_post_id TEXT REFERENCES social_posts(id),
            community_id TEXT, deleted_at TEXT DEFAULT NULL,
            view_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        , audience_profile_id TEXT);

-- index idx_social_posts_profile on social_posts
CREATE INDEX idx_social_posts_profile ON social_posts(profile_id);

-- index idx_social_posts_created on social_posts
CREATE INDEX idx_social_posts_created ON social_posts(created_at);

-- index idx_social_posts_community on social_posts
CREATE INDEX idx_social_posts_community ON social_posts(community_id);

-- table social_follows on social_follows
CREATE TABLE social_follows (
            id TEXT PRIMARY KEY, follower_profile_id TEXT NOT NULL REFERENCES social_profiles(id),
            following_profile_id TEXT NOT NULL REFERENCES social_profiles(id),
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_social_follows_pair on social_follows
CREATE UNIQUE INDEX idx_social_follows_pair ON social_follows(follower_profile_id, following_profile_id);

-- index idx_social_follows_follower on social_follows
CREATE INDEX idx_social_follows_follower ON social_follows(follower_profile_id);

-- index idx_social_follows_following on social_follows
CREATE INDEX idx_social_follows_following ON social_follows(following_profile_id);

-- table social_communities on social_communities
CREATE TABLE social_communities (
            id TEXT PRIMARY KEY, creator_profile_id TEXT NOT NULL REFERENCES social_profiles(id),
            slug TEXT NOT NULL, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
            visibility TEXT NOT NULL DEFAULT 'public',
            created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_social_communities_slug on social_communities
CREATE UNIQUE INDEX idx_social_communities_slug ON social_communities(slug);

-- index idx_social_communities_creator on social_communities
CREATE INDEX idx_social_communities_creator ON social_communities(creator_profile_id);

-- table social_community_memberships on social_community_memberships
CREATE TABLE social_community_memberships (
            id TEXT PRIMARY KEY, community_id TEXT NOT NULL REFERENCES social_communities(id),
            profile_id TEXT NOT NULL REFERENCES social_profiles(id),
            joined_at TEXT NOT NULL DEFAULT (datetime('now'))
        , role TEXT NOT NULL DEFAULT 'member');

-- index idx_social_memberships_pair on social_community_memberships
CREATE UNIQUE INDEX idx_social_memberships_pair ON social_community_memberships(community_id, profile_id);

-- index idx_social_memberships_profile on social_community_memberships
CREATE INDEX idx_social_memberships_profile ON social_community_memberships(profile_id);

-- table social_longform on social_longform
CREATE TABLE social_longform (
            id TEXT PRIMARY KEY, profile_id TEXT NOT NULL REFERENCES social_profiles(id),
            linked_agent_id TEXT REFERENCES social_linked_agents(id),
            title TEXT NOT NULL, summary TEXT NOT NULL DEFAULT '', body TEXT NOT NULL,
            format_type TEXT NOT NULL DEFAULT 'essay', visibility TEXT NOT NULL DEFAULT 'public',
            proof_state TEXT NOT NULL DEFAULT 'pending', author_mode TEXT NOT NULL DEFAULT 'person',
            created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_social_longform_profile on social_longform
CREATE INDEX idx_social_longform_profile ON social_longform(profile_id);

-- index idx_social_longform_created on social_longform
CREATE INDEX idx_social_longform_created ON social_longform(created_at);

-- table social_likes on social_likes
CREATE TABLE social_likes (
            profile_id TEXT NOT NULL, post_id TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (profile_id, post_id)
        );

-- index idx_social_likes_post on social_likes
CREATE INDEX idx_social_likes_post ON social_likes(post_id);

-- table social_bookmarks on social_bookmarks
CREATE TABLE social_bookmarks (
            profile_id TEXT NOT NULL, post_id TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (profile_id, post_id)
        );

-- index idx_social_bookmarks_post on social_bookmarks
CREATE INDEX idx_social_bookmarks_post ON social_bookmarks(post_id);

-- table social_reposts on social_reposts
CREATE TABLE social_reposts (
            profile_id TEXT NOT NULL, post_id TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (profile_id, post_id)
        );

-- index idx_social_reposts_post on social_reposts
CREATE INDEX idx_social_reposts_post ON social_reposts(post_id);

-- table pulse_drafts on pulse_drafts
CREATE TABLE pulse_drafts (
            id TEXT PRIMARY KEY, profile_id TEXT NOT NULL, body TEXT NOT NULL,
            visibility TEXT NOT NULL DEFAULT 'public', author_mode TEXT NOT NULL DEFAULT 'person',
            linked_agent_id TEXT, status TEXT NOT NULL DEFAULT 'pending',
            created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_pulse_drafts_profile on pulse_drafts
CREATE INDEX idx_pulse_drafts_profile ON pulse_drafts(profile_id);

-- index idx_pulse_drafts_status on pulse_drafts
CREATE INDEX idx_pulse_drafts_status ON pulse_drafts(status);

-- table pulse_audit_log on pulse_audit_log
CREATE TABLE pulse_audit_log (
            id TEXT PRIMARY KEY, draft_id TEXT NOT NULL, action TEXT NOT NULL,
            actor_profile_id TEXT NOT NULL, details_json TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_pulse_audit_log_draft on pulse_audit_log
CREATE INDEX idx_pulse_audit_log_draft ON pulse_audit_log(draft_id);

-- table pulse_schedules on pulse_schedules
CREATE TABLE pulse_schedules (
            id TEXT PRIMARY KEY,
            profile_id TEXT NOT NULL,
            draft_id TEXT NOT NULL,
            publish_at TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'scheduled',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_pulse_schedules_due on pulse_schedules
CREATE INDEX idx_pulse_schedules_due
            ON pulse_schedules(status, publish_at);

-- table pulse_goals on pulse_goals
CREATE TABLE pulse_goals (
            id TEXT PRIMARY KEY,
            profile_id TEXT NOT NULL,
            goal TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            plan_json TEXT NOT NULL DEFAULT '{}',
            steps_json TEXT NOT NULL DEFAULT '[]',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_pulse_goals_profile on pulse_goals
CREATE INDEX idx_pulse_goals_profile
            ON pulse_goals(profile_id, updated_at DESC);

-- index idx_pulse_goals_status on pulse_goals
CREATE INDEX idx_pulse_goals_status
            ON pulse_goals(profile_id, status);

-- table social_notifications on social_notifications
CREATE TABLE social_notifications (
            id TEXT PRIMARY KEY, recipient_profile_id TEXT NOT NULL, actor_profile_id TEXT NOT NULL,
            notification_type TEXT NOT NULL, post_id TEXT, read INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_social_notifications_recipient on social_notifications
CREATE INDEX idx_social_notifications_recipient ON social_notifications(recipient_profile_id, created_at DESC);

-- index idx_social_notifications_read on social_notifications
CREATE INDEX idx_social_notifications_read ON social_notifications(recipient_profile_id, read);

-- table social_media_objects on social_media_objects
CREATE TABLE social_media_objects (
            id TEXT PRIMARY KEY, owner_profile_id TEXT NOT NULL, filename TEXT NOT NULL,
            content_type TEXT NOT NULL, size_bytes INTEGER NOT NULL, media_type TEXT NOT NULL DEFAULT 'image',
            storage_key TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending',
            created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_social_media_objects_owner on social_media_objects
CREATE INDEX idx_social_media_objects_owner ON social_media_objects(owner_profile_id);

-- index idx_social_media_objects_status on social_media_objects
CREATE INDEX idx_social_media_objects_status ON social_media_objects(status, created_at);

-- table social_post_media on social_post_media
CREATE TABLE social_post_media (
            post_id TEXT NOT NULL, media_id TEXT NOT NULL, position INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (post_id, media_id)
        );

-- index idx_social_post_media_post on social_post_media
CREATE INDEX idx_social_post_media_post ON social_post_media(post_id);

-- index idx_social_post_media_media on social_post_media
CREATE INDEX idx_social_post_media_media ON social_post_media(media_id);

-- table webhook_events on webhook_events
CREATE TABLE webhook_events (
            id TEXT PRIMARY KEY, event_type TEXT NOT NULL, timestamp INTEGER NOT NULL,
            processed_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_webhook_events_type on webhook_events
CREATE INDEX idx_webhook_events_type ON webhook_events(event_type);

-- table accounts on accounts
CREATE TABLE accounts (
            clerk_user_id TEXT PRIMARY KEY, email TEXT NOT NULL DEFAULT '',
            display_name TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_accounts_email on accounts
CREATE INDEX idx_accounts_email ON accounts(email);

-- index idx_accounts_status on accounts
CREATE INDEX idx_accounts_status ON accounts(status);

-- table social_conversations on social_conversations
CREATE TABLE social_conversations (
            id TEXT PRIMARY KEY, created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        , direct_key TEXT, creation_key TEXT, creator_profile_id TEXT, activity_sequence INTEGER NOT NULL DEFAULT 0);

-- table social_conversation_participants on social_conversation_participants
CREATE TABLE social_conversation_participants (
            conversation_id TEXT NOT NULL, profile_id TEXT NOT NULL,
            joined_at TEXT NOT NULL DEFAULT (datetime('now')), joined_message_sequence INTEGER NOT NULL DEFAULT 0, last_read_message_sequence INTEGER NOT NULL DEFAULT 0, last_read_at TEXT, PRIMARY KEY (conversation_id, profile_id)
        );

-- index idx_social_conv_participants_profile on social_conversation_participants
CREATE INDEX idx_social_conv_participants_profile ON social_conversation_participants(profile_id);

-- table social_messages on social_messages
CREATE TABLE social_messages (
            id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL, sender_profile_id TEXT NOT NULL,
            content TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT (datetime('now')), read INTEGER NOT NULL DEFAULT 0
        , sequence INTEGER NOT NULL DEFAULT 0, client_message_id TEXT);

-- index idx_social_messages_sender on social_messages
CREATE INDEX idx_social_messages_sender ON social_messages(sender_profile_id);

-- table social_blocks on social_blocks
CREATE TABLE social_blocks (
            blocker_profile_id TEXT NOT NULL, blocked_profile_id TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')), UNIQUE (blocker_profile_id, blocked_profile_id)
        );

-- index idx_social_blocks_blocker on social_blocks
CREATE INDEX idx_social_blocks_blocker ON social_blocks(blocker_profile_id);

-- index idx_social_blocks_blocked on social_blocks
CREATE INDEX idx_social_blocks_blocked ON social_blocks(blocked_profile_id);

-- table social_mutes on social_mutes
CREATE TABLE social_mutes (
            muter_profile_id TEXT NOT NULL, muted_profile_id TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')), UNIQUE (muter_profile_id, muted_profile_id)
        );

-- index idx_social_mutes_muter on social_mutes
CREATE INDEX idx_social_mutes_muter ON social_mutes(muter_profile_id);

-- table social_reports on social_reports
CREATE TABLE social_reports (
            id TEXT PRIMARY KEY, reporter_profile_id TEXT NOT NULL, target_type TEXT NOT NULL,
            target_id TEXT NOT NULL, reason TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_social_reports_reporter on social_reports
CREATE INDEX idx_social_reports_reporter ON social_reports(reporter_profile_id);

-- index idx_social_reports_status on social_reports
CREATE INDEX idx_social_reports_status ON social_reports(status);

-- table social_posts_fts on social_posts_fts
CREATE VIRTUAL TABLE social_posts_fts USING fts5(
                post_id UNINDEXED,
                body,
                tokenize = 'porter unicode61'
            );

-- trigger social_posts_fts_ai on social_posts
CREATE TRIGGER social_posts_fts_ai AFTER INSERT ON social_posts BEGIN
            INSERT INTO social_posts_fts(post_id, body) VALUES (new.id, new.body);
         END;

-- trigger social_posts_fts_ad on social_posts
CREATE TRIGGER social_posts_fts_ad AFTER DELETE ON social_posts BEGIN
            DELETE FROM social_posts_fts WHERE post_id = old.id;
         END;

-- trigger social_posts_fts_au on social_posts
CREATE TRIGGER social_posts_fts_au AFTER UPDATE OF body ON social_posts BEGIN
            DELETE FROM social_posts_fts WHERE post_id = old.id;
            INSERT INTO social_posts_fts(post_id, body) VALUES (new.id, new.body);
         END;

-- table social_pages on social_pages
CREATE TABLE social_pages (
            id TEXT PRIMARY KEY,
            owner_profile_id TEXT NOT NULL REFERENCES social_profiles(id),
            kind TEXT NOT NULL DEFAULT 'brand',
            slug TEXT NOT NULL,
            display_name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            avatar_url TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_social_pages_slug on social_pages
CREATE UNIQUE INDEX idx_social_pages_slug ON social_pages(slug);

-- index idx_social_pages_owner on social_pages
CREATE INDEX idx_social_pages_owner ON social_pages(owner_profile_id);

-- table social_page_follows on social_page_follows
CREATE TABLE social_page_follows (
            id TEXT PRIMARY KEY,
            page_id TEXT NOT NULL REFERENCES social_pages(id),
            follower_profile_id TEXT NOT NULL REFERENCES social_profiles(id),
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_social_page_follows_pair on social_page_follows
CREATE UNIQUE INDEX idx_social_page_follows_pair
            ON social_page_follows(page_id, follower_profile_id);

-- index idx_social_page_follows_follower on social_page_follows
CREATE INDEX idx_social_page_follows_follower
            ON social_page_follows(follower_profile_id);

-- index idx_social_page_follows_page on social_page_follows
CREATE INDEX idx_social_page_follows_page
            ON social_page_follows(page_id);

-- table social_media_shelves on social_media_shelves
CREATE TABLE social_media_shelves (
            id TEXT PRIMARY KEY,
            owner_profile_id TEXT NOT NULL REFERENCES social_profiles(id),
            title TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_social_media_shelves_owner on social_media_shelves
CREATE INDEX idx_social_media_shelves_owner
            ON social_media_shelves(owner_profile_id, created_at DESC);

-- table social_profile_prefs on social_profile_prefs
CREATE TABLE social_profile_prefs (
            profile_id TEXT PRIMARY KEY,
            dm_policy TEXT NOT NULL DEFAULT 'verified',
            discoverable_by_contact INTEGER NOT NULL DEFAULT 0,
            show_in_search INTEGER NOT NULL DEFAULT 1,
            protected_posts INTEGER NOT NULL DEFAULT 0,
            profile_visibility TEXT NOT NULL DEFAULT 'public',
            allow_agent_dms INTEGER NOT NULL DEFAULT 0,
            allow_agent_mentions INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- table social_community_invites on social_community_invites
CREATE TABLE social_community_invites (
            id TEXT PRIMARY KEY,
            community_id TEXT NOT NULL REFERENCES social_communities(id) ON DELETE CASCADE,
            created_by_profile_id TEXT NOT NULL REFERENCES social_profiles(id),
            token_hash TEXT NOT NULL UNIQUE,
            max_uses INTEGER,
            use_count INTEGER NOT NULL DEFAULT 0,
            expires_at TEXT,
            revoked_at TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_social_community_invites_community on social_community_invites
CREATE INDEX idx_social_community_invites_community
            ON social_community_invites(community_id, created_at DESC);

-- index idx_social_community_invites_token_hash on social_community_invites
CREATE UNIQUE INDEX idx_social_community_invites_token_hash
            ON social_community_invites(token_hash);

-- table social_live_sessions on social_live_sessions
CREATE TABLE social_live_sessions (
            id TEXT PRIMARY KEY,
            owner_profile_id TEXT NOT NULL REFERENCES social_profiles(id),
            title TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            phase TEXT NOT NULL DEFAULT 'preview',
            ingest_url TEXT,
            playback_url TEXT,
            provider TEXT NOT NULL DEFAULT 'none',
            started_at TEXT,
            ended_at TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_social_live_sessions_owner on social_live_sessions
CREATE INDEX idx_social_live_sessions_owner
            ON social_live_sessions(owner_profile_id, created_at DESC);

-- index idx_social_live_sessions_phase on social_live_sessions
CREATE INDEX idx_social_live_sessions_phase
            ON social_live_sessions(phase, updated_at DESC);

-- table social_x402_receipts on social_x402_receipts
CREATE TABLE social_x402_receipts (
            id TEXT PRIMARY KEY,
            idempotency_key TEXT NOT NULL UNIQUE,
            payload_hash TEXT NOT NULL,
            amount TEXT,
            network TEXT NOT NULL,
            status TEXT NOT NULL,
            mode TEXT NOT NULL DEFAULT 'shape_only',
            note TEXT,
            raw_response TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

-- index idx_social_x402_receipts_status on social_x402_receipts
CREATE INDEX idx_social_x402_receipts_status
            ON social_x402_receipts(status, created_at DESC);

-- index idx_social_x402_receipts_hash on social_x402_receipts
CREATE INDEX idx_social_x402_receipts_hash
            ON social_x402_receipts(payload_hash);

-- table social_ws_tickets on social_ws_tickets
CREATE TABLE social_ws_tickets (
            token_hash TEXT PRIMARY KEY,
            clerk_user_id TEXT NOT NULL,
            expires_at INTEGER NOT NULL,
            consumed_at INTEGER,
            created_at INTEGER NOT NULL DEFAULT (unixepoch())
        );

-- index idx_social_ws_tickets_expiry on social_ws_tickets
CREATE INDEX idx_social_ws_tickets_expiry
            ON social_ws_tickets(expires_at);

-- trigger social_posts_visibility_insert on social_posts
CREATE TRIGGER social_posts_visibility_insert
        BEFORE INSERT ON social_posts
        WHEN NEW.visibility NOT IN ('public', 'followers', 'mutuals', 'guild', 'circle', 'author-only')
        BEGIN SELECT RAISE(ABORT, 'invalid social post visibility'); END;

-- trigger social_posts_visibility_update on social_posts
CREATE TRIGGER social_posts_visibility_update
        BEFORE UPDATE OF visibility ON social_posts
        WHEN NEW.visibility NOT IN ('public', 'followers', 'mutuals', 'guild', 'circle', 'author-only')
        BEGIN SELECT RAISE(ABORT, 'invalid social post visibility'); END;

-- trigger social_posts_audience_insert on social_posts
CREATE TRIGGER social_posts_audience_insert
        BEFORE INSERT ON social_posts
        WHEN (
            NEW.reply_to_post_id IS NULL
            AND (
                NULLIF(NEW.audience_profile_id, '') IS NULL
                OR NEW.audience_profile_id != NEW.profile_id
            )
        ) OR (
            NEW.reply_to_post_id IS NOT NULL
            AND NOT EXISTS (
                SELECT 1
                  FROM social_posts parent
                 WHERE parent.id = NEW.reply_to_post_id
                   AND parent.id != NEW.id
                   AND NEW.audience_profile_id = parent.audience_profile_id
                   AND NEW.visibility = parent.visibility
                   AND NEW.community_id IS parent.community_id
            )
        )
        BEGIN SELECT RAISE(ABORT, 'invalid social reply audience'); END;

-- trigger social_posts_audience_update on social_posts
CREATE TRIGGER social_posts_audience_update
        BEFORE UPDATE OF profile_id, reply_to_post_id, visibility, community_id, audience_profile_id
        ON social_posts
        WHEN (
            NEW.reply_to_post_id IS NULL
            AND (
                NULLIF(NEW.audience_profile_id, '') IS NULL
                OR NEW.audience_profile_id != NEW.profile_id
            )
        ) OR (
            NEW.reply_to_post_id IS NOT NULL
            AND NOT EXISTS (
                SELECT 1
                  FROM social_posts parent
                 WHERE parent.id = NEW.reply_to_post_id
                   AND parent.id != NEW.id
                   AND NEW.audience_profile_id = parent.audience_profile_id
                   AND NEW.visibility = parent.visibility
                   AND NEW.community_id IS parent.community_id
            )
        )
        BEGIN SELECT RAISE(ABORT, 'invalid social reply audience'); END;

-- trigger social_posts_reply_parent_immutable on social_posts
CREATE TRIGGER social_posts_reply_parent_immutable
        BEFORE UPDATE OF reply_to_post_id ON social_posts
        WHEN NEW.reply_to_post_id IS NOT OLD.reply_to_post_id
        BEGIN SELECT RAISE(ABORT, 'social reply parent is immutable'); END;

-- trigger social_posts_audience_with_replies_immutable on social_posts
CREATE TRIGGER social_posts_audience_with_replies_immutable
        BEFORE UPDATE OF profile_id, visibility, community_id, audience_profile_id ON social_posts
        WHEN EXISTS (
            SELECT 1 FROM social_posts child WHERE child.reply_to_post_id = OLD.id
        ) AND (
            NEW.profile_id IS NOT OLD.profile_id
            OR NEW.visibility IS NOT OLD.visibility
            OR NEW.community_id IS NOT OLD.community_id
            OR NEW.audience_profile_id IS NOT OLD.audience_profile_id
        )
        BEGIN SELECT RAISE(ABORT, 'social post audience with replies is immutable'); END;

-- trigger social_longform_visibility_insert on social_longform
CREATE TRIGGER social_longform_visibility_insert
        BEFORE INSERT ON social_longform
        WHEN NEW.visibility NOT IN ('public', 'followers', 'mutuals', 'author-only')
        BEGIN SELECT RAISE(ABORT, 'invalid social longform visibility'); END;

-- trigger social_longform_visibility_update on social_longform
CREATE TRIGGER social_longform_visibility_update
        BEFORE UPDATE OF visibility ON social_longform
        WHEN NEW.visibility NOT IN ('public', 'followers', 'mutuals', 'author-only')
        BEGIN SELECT RAISE(ABORT, 'invalid social longform visibility'); END;

-- trigger pulse_drafts_visibility_insert on pulse_drafts
CREATE TRIGGER pulse_drafts_visibility_insert
        BEFORE INSERT ON pulse_drafts
        WHEN NEW.visibility NOT IN ('public', 'followers', 'mutuals', 'author-only')
        BEGIN SELECT RAISE(ABORT, 'invalid Pulse draft visibility'); END;

-- trigger pulse_drafts_visibility_update on pulse_drafts
CREATE TRIGGER pulse_drafts_visibility_update
        BEFORE UPDATE OF visibility ON pulse_drafts
        WHEN NEW.visibility NOT IN ('public', 'followers', 'mutuals', 'author-only')
        BEGIN SELECT RAISE(ABORT, 'invalid Pulse draft visibility'); END;

-- table social_follow_requests on social_follow_requests
CREATE TABLE social_follow_requests (
                id TEXT PRIMARY KEY,
                requester_profile_id TEXT NOT NULL REFERENCES social_profiles(id),
                target_profile_id TEXT NOT NULL REFERENCES social_profiles(id),
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(requester_profile_id, target_profile_id)
            );

-- index idx_social_follow_requests_target on social_follow_requests
CREATE INDEX idx_social_follow_requests_target
                ON social_follow_requests(target_profile_id, status, created_at DESC);

-- index idx_social_follow_requests_requester on social_follow_requests
CREATE INDEX idx_social_follow_requests_requester
                ON social_follow_requests(requester_profile_id, status, created_at DESC);

-- trigger social_follow_requests_status_insert on social_follow_requests
CREATE TRIGGER social_follow_requests_status_insert
            BEFORE INSERT ON social_follow_requests
            WHEN NEW.status NOT IN ('pending', 'accepted', 'rejected')
            BEGIN SELECT RAISE(ABORT, 'invalid follow request status'); END;

-- trigger social_follow_requests_status_update on social_follow_requests
CREATE TRIGGER social_follow_requests_status_update
            BEFORE UPDATE OF status ON social_follow_requests
            WHEN NEW.status NOT IN ('pending', 'accepted', 'rejected')
            BEGIN SELECT RAISE(ABORT, 'invalid follow request status'); END;

-- index idx_social_messages_conversation_sequence on social_messages
CREATE UNIQUE INDEX idx_social_messages_conversation_sequence
                ON social_messages(conversation_id, sequence);

-- index idx_social_messages_conversation_created on social_messages
CREATE INDEX idx_social_messages_conversation_created
                ON social_messages(conversation_id, created_at DESC, id DESC);

-- index idx_social_messages_client_id on social_messages
CREATE UNIQUE INDEX idx_social_messages_client_id
                ON social_messages(sender_profile_id, client_message_id)
                WHERE client_message_id IS NOT NULL;

-- index idx_social_conversations_direct_key on social_conversations
CREATE UNIQUE INDEX idx_social_conversations_direct_key
                ON social_conversations(direct_key) WHERE direct_key IS NOT NULL;

-- index idx_social_conversations_creation_key on social_conversations
CREATE UNIQUE INDEX idx_social_conversations_creation_key
                ON social_conversations(creator_profile_id, creation_key)
                WHERE creation_key IS NOT NULL;

-- index idx_social_participants_unread on social_conversation_participants
CREATE INDEX idx_social_participants_unread
                ON social_conversation_participants(
                    profile_id, conversation_id, last_read_message_sequence
                );

-- trigger social_messages_integrity_insert on social_messages
CREATE TRIGGER social_messages_integrity_insert
            BEFORE INSERT ON social_messages
            WHEN NEW.sequence <= 0
              OR NEW.client_message_id IS NULL
              OR length(NEW.client_message_id) < 8
              OR length(NEW.client_message_id) > 128
              OR length(NEW.content) < 1
              OR length(NEW.content) > 4000
              OR length(CAST(NEW.content AS BLOB)) > 16384
              OR instr(NEW.content, char(0)) != 0
            BEGIN SELECT RAISE(ABORT, 'invalid social message'); END;

-- table social_conversation_activity_clock on social_conversation_activity_clock
CREATE TABLE social_conversation_activity_clock (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                next_sequence INTEGER NOT NULL CHECK(next_sequence > 0)
             ) WITHOUT ROWID;

-- index idx_social_conversations_activity on social_conversations
CREATE UNIQUE INDEX idx_social_conversations_activity
                 ON social_conversations(activity_sequence DESC, id DESC);

-- index idx_social_conv_participants_profile_conversation on social_conversation_participants
CREATE INDEX idx_social_conv_participants_profile_conversation
                 ON social_conversation_participants(profile_id, conversation_id);

-- trigger social_profile_prefs_dm_policy_insert on social_profile_prefs
CREATE TRIGGER social_profile_prefs_dm_policy_insert
             BEFORE INSERT ON social_profile_prefs
             WHEN NEW.dm_policy NOT IN ('everyone', 'verified', 'following', 'mutuals', 'nobody')
             BEGIN SELECT RAISE(ABORT, 'invalid social DM policy'); END;

-- trigger social_profile_prefs_dm_policy_update on social_profile_prefs
CREATE TRIGGER social_profile_prefs_dm_policy_update
             BEFORE UPDATE OF dm_policy ON social_profile_prefs
             WHEN NEW.dm_policy NOT IN ('everyone', 'verified', 'following', 'mutuals', 'nobody')
             BEGIN SELECT RAISE(ABORT, 'invalid social DM policy'); END;

-- table social_message_request_clock on social_message_request_clock
CREATE TABLE social_message_request_clock (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                next_sequence INTEGER NOT NULL CHECK(next_sequence > 0)
             ) WITHOUT ROWID;

-- table social_message_requests on social_message_requests
CREATE TABLE social_message_requests (
                id TEXT PRIMARY KEY,
                sender_profile_id TEXT NOT NULL REFERENCES social_profiles(id) ON DELETE CASCADE,
                recipient_profile_id TEXT NOT NULL REFERENCES social_profiles(id) ON DELETE CASCADE,
                client_request_id TEXT NOT NULL,
                content TEXT NOT NULL,
                content_fingerprint TEXT NOT NULL,
                state TEXT NOT NULL DEFAULT 'pending'
                    CHECK(state IN ('pending', 'accepted', 'declined', 'spam',
                                    'cancelled', 'expired', 'blocked')),
                bucket TEXT NOT NULL DEFAULT 'inbox'
                    CHECK(bucket IN ('inbox', 'spam')),
                risk_score INTEGER NOT NULL DEFAULT 0
                    CHECK(risk_score BETWEEN 0 AND 100),
                risk_reasons_json TEXT NOT NULL DEFAULT '[]',
                activity_sequence INTEGER NOT NULL CHECK(activity_sequence > 0),
                conversation_id TEXT REFERENCES social_conversations(id),
                accepted_message_id TEXT REFERENCES social_messages(id),
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                resolved_at TEXT,
                resolver_profile_id TEXT,
                CHECK(sender_profile_id != recipient_profile_id),
                CHECK(length(content_fingerprint) = 64
                    AND content_fingerprint NOT GLOB '*[^0-9a-f]*'),
                CHECK(json_valid(risk_reasons_json) AND json_type(risk_reasons_json) = 'array'),
                CHECK(
                    (state = 'pending' AND resolved_at IS NULL
                        AND resolver_profile_id IS NULL AND conversation_id IS NULL
                        AND accepted_message_id IS NULL)
                    OR (state = 'accepted' AND resolved_at IS NOT NULL
                        AND resolver_profile_id = recipient_profile_id
                        AND conversation_id IS NOT NULL AND accepted_message_id IS NOT NULL)
                    OR (state IN ('declined', 'spam') AND resolved_at IS NOT NULL
                        AND resolver_profile_id = recipient_profile_id
                        AND conversation_id IS NULL AND accepted_message_id IS NULL)
                    OR (state = 'blocked' AND resolved_at IS NOT NULL
                        AND resolver_profile_id IN (sender_profile_id, recipient_profile_id)
                        AND conversation_id IS NULL AND accepted_message_id IS NULL)
                    OR (state = 'cancelled' AND resolved_at IS NOT NULL
                        AND resolver_profile_id = sender_profile_id
                        AND conversation_id IS NULL AND accepted_message_id IS NULL)
                    OR (state = 'expired' AND resolved_at IS NOT NULL
                        AND resolver_profile_id IS NULL
                        AND conversation_id IS NULL AND accepted_message_id IS NULL)
                )
             );

-- index idx_social_message_requests_sender_client on social_message_requests
CREATE UNIQUE INDEX idx_social_message_requests_sender_client
                ON social_message_requests(sender_profile_id, client_request_id);

-- index idx_social_message_requests_pending_pair on social_message_requests
CREATE UNIQUE INDEX idx_social_message_requests_pending_pair
                ON social_message_requests(sender_profile_id, recipient_profile_id)
                WHERE state = 'pending';

-- index idx_social_message_requests_activity on social_message_requests
CREATE UNIQUE INDEX idx_social_message_requests_activity
                ON social_message_requests(activity_sequence DESC, id DESC);

-- index idx_social_message_requests_recipient_inbox on social_message_requests
CREATE INDEX idx_social_message_requests_recipient_inbox
                ON social_message_requests(recipient_profile_id, bucket, state,
                                           activity_sequence DESC, id DESC);

-- index idx_social_message_requests_sender_state on social_message_requests
CREATE INDEX idx_social_message_requests_sender_state
                ON social_message_requests(sender_profile_id, state, created_at DESC, id DESC);

-- index idx_social_message_requests_fingerprint_recent on social_message_requests
CREATE INDEX idx_social_message_requests_fingerprint_recent
                ON social_message_requests(sender_profile_id, content_fingerprint, created_at DESC);

-- trigger social_message_requests_integrity_insert on social_message_requests
CREATE TRIGGER social_message_requests_integrity_insert
             BEFORE INSERT ON social_message_requests
             WHEN length(NEW.sender_profile_id) < 1 OR length(NEW.sender_profile_id) > 128
               OR length(NEW.recipient_profile_id) < 1 OR length(NEW.recipient_profile_id) > 128
               OR length(NEW.client_request_id) < 8 OR length(NEW.client_request_id) > 128
               OR length(NEW.content) < 1 OR length(NEW.content) > 4000
               OR length(CAST(NEW.content AS BLOB)) > 16384
               OR instr(NEW.content, char(0)) != 0
             BEGIN SELECT RAISE(ABORT, 'invalid social message request'); END;

-- trigger social_message_requests_immutable_update on social_message_requests
CREATE TRIGGER social_message_requests_immutable_update
             BEFORE UPDATE ON social_message_requests
             WHEN OLD.state != 'pending'
               OR NEW.id != OLD.id
               OR NEW.sender_profile_id != OLD.sender_profile_id
               OR NEW.recipient_profile_id != OLD.recipient_profile_id
               OR NEW.client_request_id != OLD.client_request_id
               OR NEW.content != OLD.content
               OR NEW.content_fingerprint != OLD.content_fingerprint
               OR NEW.bucket != OLD.bucket
               OR NEW.risk_score != OLD.risk_score
               OR NEW.risk_reasons_json != OLD.risk_reasons_json
               OR NEW.activity_sequence != OLD.activity_sequence
               OR NEW.created_at != OLD.created_at
             BEGIN SELECT RAISE(ABORT, 'immutable social message request'); END;

-- trigger social_message_requests_accept_links on social_message_requests
CREATE TRIGGER social_message_requests_accept_links
             BEFORE UPDATE ON social_message_requests
             WHEN NEW.state = 'accepted' AND (
                NOT EXISTS(SELECT 1 FROM social_messages message
                    WHERE message.id = NEW.accepted_message_id
                      AND message.conversation_id = NEW.conversation_id
                      AND message.sender_profile_id = NEW.sender_profile_id
                      AND message.content = NEW.content)
                OR NOT EXISTS(SELECT 1 FROM social_conversation_participants participant
                    WHERE participant.conversation_id = NEW.conversation_id
                      AND participant.profile_id = NEW.sender_profile_id)
                OR NOT EXISTS(SELECT 1 FROM social_conversation_participants participant
                    WHERE participant.conversation_id = NEW.conversation_id
                      AND participant.profile_id = NEW.recipient_profile_id)
             ) BEGIN SELECT RAISE(ABORT, 'invalid accepted message request links'); END;

-- table social_direct_message_starts on social_direct_message_starts
CREATE TABLE social_direct_message_starts (
                sender_profile_id TEXT NOT NULL REFERENCES social_profiles(id) ON DELETE CASCADE,
                client_request_id TEXT NOT NULL,
                recipient_profile_id TEXT NOT NULL REFERENCES social_profiles(id) ON DELETE CASCADE,
                content_fingerprint TEXT NOT NULL CHECK(length(content_fingerprint) = 64
                    AND content_fingerprint NOT GLOB '*[^0-9a-f]*'),
                outcome_type TEXT NOT NULL CHECK(outcome_type IN ('request', 'message')),
                outcome_id TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                PRIMARY KEY(sender_profile_id, client_request_id)
             ) WITHOUT ROWID;

-- index idx_social_direct_message_starts_quota on social_direct_message_starts
CREATE INDEX idx_social_direct_message_starts_quota
                ON social_direct_message_starts(sender_profile_id, created_at DESC);

-- trigger social_direct_message_starts_outcome on social_direct_message_starts
CREATE TRIGGER social_direct_message_starts_outcome
             BEFORE INSERT ON social_direct_message_starts
             WHEN (NEW.outcome_type = 'request'
                    AND NOT EXISTS(SELECT 1 FROM social_message_requests WHERE id = NEW.outcome_id))
                OR (NEW.outcome_type = 'message'
                    AND NOT EXISTS(SELECT 1 FROM social_messages WHERE id = NEW.outcome_id))
             BEGIN SELECT RAISE(ABORT, 'invalid direct message start outcome'); END;

-- Singleton allocators are data-backed schema state. They are initialized in
-- the baseline rather than copied from a mixed source database.
INSERT INTO social_conversation_activity_clock(singleton, next_sequence) VALUES (1, 1);
INSERT INTO social_message_request_clock(singleton, next_sequence) VALUES (1, 1);
