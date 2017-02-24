typedef void* video_player_ptr;

#define AMPLAYER_ERROR_INVALID_COMMAND 		1
#define AMPLAYER_ERROR_NONE 			0
#define AMPLAYER_ERROR_UNKNOWN 			-1
#define AMPLAYER_ERROR_DISCONNECTED		-2
#define AMPLAYER_ERROR_LIBAV_DISCONNECTED	-3
#define AMPLAYER_ERROR_LIBAV_INTERNAL		-4
#define AMPLAYER_ERROR_VIDEO_DECODING		-5
#define AMPLAYER_ERROR_NO_HEVC_STREAM		-6
#define AMPLAYER_ERROR_X11_DL_OPEN		-7
#define AMPLAYER_ERROR_X11_INTERNAL		-8
#define AMPLAYER_BUG				-42
#define AMPLAYER_UNREACHABLE			-43
#define AMPLAYER_ERROR_SHUTDOWN			-64

// Créé une instance du lecteur
//
// Renvoie NULL si une erreur s'est produite,
// le pointeur du lecteur sinon
video_player_ptr aml_video_player_create();

// Charge la vidéo depuis l'URL donnée
// l'URL peut être une adresse web délivrant
// du mp4 valide,
// ou un chemin sur le système de fichier courant
//
// Renvoie <0 en cas d'erreur
int aml_video_player_load(video_player_ptr, const char* video_url);

// Montre le lecteur vidéo en premier plan
//
// Renvoie <0 en cas d'erreur
int aml_video_player_show(video_player_ptr);

// Cache le lecteur vidéo
//
// Renvoie <0 en cas d'erreur
int aml_video_player_hide(video_player_ptr);

// Commence la lecture du lecteur vidéo
//
// Renvoie <0 en cas d'erreur
int aml_video_player_play(video_player_ptr);

// Active la pause du lecteur vidéo
//
// Renvoie <0 en cas d'erreur
int aml_video_player_pause(video_player_ptr);

// Essaie de mettre la position du lecteur à la seconde t
//
// Renvoie <0 en cas d'erreur
int aml_video_player_seek(video_player_ptr, float t);

// Tente de redimensionner le lecteur à la taille donnée
//
// Renvoie <0 en cas d'erreur
int aml_video_player_resize(video_player_ptr, unsigned int width, unsigned int height);

// Tente de déplacer le coin haut-gauche du lecteur 
// à la position (x, y), relativement à la fenêtre
// "racine" X11  (qui devrait être le point en haut 
// à gauche de l'écran)
//
// Renvoie <0 en cas d'erreur
int aml_video_player_set_pos(video_player_ptr,int x, int y);

// Active/désactive le plein écran du lecteur
// 
// fullscreen == 0: désactive le fullscreen
// fullscreen > 0: active le fullscreen
//
// Renvoie <0 en cas d'erreur
int aml_video_player_set_fullscreen(video_player_ptr, int fullscreen);

// // Bloque l'appel jusqu'à ce que la vidéo en cours
// // de lecture arrive à la fin de son flux
// // 
void aml_video_player_wait_until_end(video_player_ptr);

// Détruit l'instance du lecteur vidéo. Utiliser
// ce pointeur par la suite est un comportement
// indéfini.
//
int aml_video_player_destroy(video_player_ptr);
